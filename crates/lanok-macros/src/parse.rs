//! Parsing for the `protocol!` declaration.
//!
//! The grammar is deliberately small. Everything it accepts maps onto something
//! a peer or an artifact actually needs, so there is no syntax whose only
//! effect is to be written down.
//!
//! ```text
//! name    = "echo";
//! version = "1.1";
//! min     = "1.0";                       // optional, defaults to MAJOR.0
//! capabilities { tools, ui_ask };        // optional
//!
//! /// Doc comments ride along into meta.json.
//! initiator fn echo(EchoParams) -> EchoResult;
//! initiator notify "log/line" log_line(LogParams);
//! responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";
//! ```

use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Ident, LitStr, Token, Type, braced, parenthesized};

/// Which side sends a method.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Initiator,
    Responder,
}

impl Direction {
    pub fn meta_path(self) -> proc_macro2::TokenStream {
        match self {
            Direction::Initiator => quote::quote!(::lanok::Direction::Initiator),
            Direction::Responder => quote::quote!(::lanok::Direction::Responder),
        }
    }
}

pub struct Method {
    pub doc: String,
    pub direction: Direction,
    /// `true` for a request, `false` for a notification.
    pub expects_response: bool,
    /// The name on the wire.
    pub wire_name: String,
    /// The Rust identifier for stubs and handlers.
    pub ident: Ident,
    pub params: Option<Type>,
    pub result: Option<Type>,
    pub requires: Option<String>,
    pub span: Span,
}

pub struct Protocol {
    pub name: String,
    pub version: (u32, u32),
    pub min_version: (u32, u32),
    pub capabilities: Vec<String>,
    pub methods: Vec<Method>,
}

fn parse_version(literal: &LitStr) -> syn::Result<(u32, u32)> {
    let text = literal.value();
    let invalid = || {
        syn::Error::new(
            literal.span(),
            format!("`{text}` is not a MAJOR.MINOR protocol version"),
        )
    };
    let (major, minor) = text.split_once('.').ok_or_else(invalid)?;
    if minor.contains('.') {
        return Err(invalid());
    }
    Ok((
        major.parse().map_err(|_| invalid())?,
        minor.parse().map_err(|_| invalid())?,
    ))
}

/// Collect `///` lines into one paragraph for meta.json.
fn doc_of(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(text),
                ..
            }) = &nv.value
        {
            lines.push(text.value().trim().to_string());
        }
    }
    lines.join(" ").trim().to_string()
}

impl Parse for Protocol {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name: Option<String> = None;
        let mut version: Option<(u32, u32)> = None;
        let mut min_version: Option<(u32, u32)> = None;
        let mut capabilities: Vec<String> = Vec::new();
        let mut methods: Vec<Method> = Vec::new();

        while !input.is_empty() {
            let attrs = input.call(Attribute::parse_outer)?;

            // Header entries: `name = "..."`, `version = "..."`, `min = "..."`.
            if input.peek(Ident) && input.peek2(Token![=]) {
                let key: Ident = input.parse()?;
                let _: Token![=] = input.parse()?;
                let value: LitStr = input.parse()?;
                let _: Token![;] = input.parse()?;
                match key.to_string().as_str() {
                    "name" => name = Some(value.value()),
                    "version" => version = Some(parse_version(&value)?),
                    "min" => min_version = Some(parse_version(&value)?),
                    other => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!(
                                "unknown setting `{other}`; expected `name`, `version`, or `min`"
                            ),
                        ));
                    }
                }
                continue;
            }

            // `capabilities { a, b };`
            let is_capabilities = input.peek(Ident)
                && input.peek2(syn::token::Brace)
                && input
                    .fork()
                    .parse::<Ident>()
                    .map(|i| i == "capabilities")
                    .unwrap_or(false);
            if is_capabilities {
                let _: Ident = input.parse()?;
                let inner;
                braced!(inner in input);
                let tokens = Punctuated::<Ident, Token![,]>::parse_terminated(&inner)?;
                capabilities.extend(tokens.into_iter().map(|i| i.to_string()));
                // The trailing semicolon is optional: a braced block reads
                // complete without one, and requiring it is a papercut.
                if input.peek(Token![;]) {
                    let _: Token![;] = input.parse()?;
                }
                continue;
            }

            methods.push(parse_method(input, doc_of(&attrs))?);
        }

        let name = name
            .ok_or_else(|| syn::Error::new(Span::call_site(), "`name = \"...\";` is required"))?;
        let version = version.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "`version = \"MAJOR.MINOR\";` is required",
            )
        })?;
        let min_version = min_version.unwrap_or((version.0, 0));

        validate(&name, version, min_version, &capabilities, &methods)?;

        Ok(Protocol {
            name,
            version,
            min_version,
            capabilities,
            methods,
        })
    }
}

fn parse_method(input: ParseStream, doc: String) -> syn::Result<Method> {
    let keyword: Ident = input.parse()?;
    let span = keyword.span();
    let direction = match keyword.to_string().as_str() {
        "initiator" => Direction::Initiator,
        "responder" => Direction::Responder,
        other => {
            return Err(syn::Error::new(
                span,
                format!("expected `initiator` or `responder`, found `{other}`"),
            ));
        }
    };

    // `fn` is a Rust keyword, so it has to be parsed as a raw identifier. It is
    // worth the small cost: `initiator fn echo(...)` reads like the call it
    // generates, which is the point of a declaration macro.
    let kind: Ident = input.call(Ident::parse_any)?;
    let expects_response = match kind.to_string().as_str() {
        "fn" | "request" => true,
        "notify" => false,
        other => {
            return Err(syn::Error::new(
                kind.span(),
                format!("expected `fn` or `notify`, found `{other}`"),
            ));
        }
    };

    // An explicit wire name comes first when the Rust name cannot spell it.
    let explicit_wire: Option<LitStr> = if input.peek(LitStr) {
        Some(input.parse()?)
    } else {
        None
    };
    let ident: Ident = input.parse()?;
    let wire_name = explicit_wire
        .map(|l| l.value())
        .unwrap_or_else(|| ident.to_string());

    let inner;
    parenthesized!(inner in input);
    let params: Option<Type> = if inner.is_empty() {
        None
    } else {
        Some(inner.parse()?)
    };

    let result: Option<Type> = if input.peek(Token![->]) {
        let _: Token![->] = input.parse()?;
        Some(input.parse()?)
    } else {
        None
    };

    let requires: Option<String> = if input.peek(Ident) {
        let word: Ident = input.parse()?;
        if word != "requires" {
            return Err(syn::Error::new(
                word.span(),
                format!("expected `requires` or `;`, found `{word}`"),
            ));
        }
        let token: LitStr = input.parse()?;
        Some(token.value())
    } else {
        None
    };

    let _: Token![;] = input.parse()?;

    if !expects_response && result.is_some() {
        return Err(syn::Error::new(
            span,
            "a notification cannot have a result: it carries no id, so there is nothing to \
             answer. Declare it as `fn` if the sender needs a reply.",
        ));
    }

    Ok(Method {
        doc,
        direction,
        expects_response,
        wire_name,
        ident,
        params,
        result,
        requires,
        span,
    })
}

/// Reject declarations that would compile into something subtly wrong.
fn validate(
    name: &str,
    version: (u32, u32),
    min_version: (u32, u32),
    capabilities: &[String],
    methods: &[Method],
) -> syn::Result<()> {
    let at = |span| syn::Error::new(span, String::new());
    let _ = at;

    if name.is_empty() {
        return Err(syn::Error::new(
            Span::call_site(),
            "`name` must not be empty: it identifies the protocol in meta.json and in errors",
        ));
    }

    if min_version.0 != version.0 {
        return Err(syn::Error::new(
            Span::call_site(),
            format!(
                "`min` ({}.{}) must share a major with `version` ({}.{}): peers across a major \
                 cannot talk, so a lower major cannot be a supported minimum",
                min_version.0, min_version.1, version.0, version.1
            ),
        ));
    }
    if min_version > version {
        return Err(syn::Error::new(
            Span::call_site(),
            "`min` must not be newer than `version`: a build cannot require a peer newer than \
             itself",
        ));
    }

    for (index, method) in methods.iter().enumerate() {
        if methods[..index]
            .iter()
            .any(|m| m.wire_name == method.wire_name)
        {
            return Err(syn::Error::new(
                method.span,
                format!("method `{}` is declared twice", method.wire_name),
            ));
        }
        // `initialize` and `initialized` may be declared. A protocol whose
        // handshake payloads are its own (mira answers with its eval catalogue,
        // MCP with serverInfo and a nested capabilities object) has to be able
        // to type them. Declaring them means owning them: the peer only
        // intercepts the handshake when it was configured to serve one.
        // A capability that is never declared is almost always a typo, and it
        // fails closed at runtime: the method silently becomes unavailable.
        if let Some(token) = &method.requires
            && !capabilities.iter().any(|c| c == token)
        {
            return Err(syn::Error::new(
                method.span,
                format!(
                    "`{}` requires the capability `{token}`, which is not in the \
                     `capabilities {{ ... }}` block",
                    method.wire_name
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<Protocol> {
        syn::parse2::<Protocol>(tokens)
    }

    fn error(tokens: proc_macro2::TokenStream) -> String {
        parse(tokens)
            .err()
            .expect("declaration should be rejected")
            .to_string()
    }

    #[test]
    fn accepts_a_full_declaration() {
        let protocol = parse(quote! {
            name    = "echo";
            version = "1.2";
            min     = "1.1";

            /// Uppercase some text.
            initiator fn echo(EchoParams) -> EchoResult;
            responder notify "echo/progress" progress(ProgressParams);
            responder fn "ui/ask" ui_ask(AskParams) -> AskResult requires "ui_ask";
            initiator fn ping();

            capabilities { ui_ask }
        })
        .unwrap();

        assert_eq!(protocol.name, "echo");
        assert_eq!(protocol.version, (1, 2));
        assert_eq!(protocol.min_version, (1, 1));
        assert_eq!(protocol.capabilities, ["ui_ask"]);
        assert_eq!(protocol.methods.len(), 4);

        let echo = &protocol.methods[0];
        assert_eq!(echo.wire_name, "echo");
        assert_eq!(echo.doc, "Uppercase some text.");
        assert!(echo.expects_response);
        assert!(echo.direction == Direction::Initiator);

        // An explicit wire name is kept while the Rust identifier stays legal.
        let progress = &protocol.methods[1];
        assert_eq!(progress.wire_name, "echo/progress");
        assert_eq!(progress.ident.to_string(), "progress");
        assert!(!progress.expects_response);

        // A method with neither params nor result is legal and stays typed.
        let ping = &protocol.methods[3];
        assert!(ping.params.is_none() && ping.result.is_none());
    }

    #[test]
    fn min_defaults_to_the_majors_first_minor() {
        let protocol = parse(quote! {
            name = "p";
            version = "2.7";
        })
        .unwrap();
        assert_eq!(protocol.min_version, (2, 0));
    }

    #[test]
    fn requires_a_name_and_a_version() {
        assert!(error(quote! { version = "1.0"; }).contains("name"));
        assert!(error(quote! { name = "p"; }).contains("version"));
    }

    #[test]
    fn rejects_a_malformed_version() {
        assert!(error(quote! { name = "p"; version = "1"; }).contains("MAJOR.MINOR"));
        assert!(error(quote! { name = "p"; version = "1.2.3"; }).contains("MAJOR.MINOR"));
    }

    #[test]
    fn rejects_a_min_that_cannot_be_a_minimum() {
        // A different major cannot talk at all, so it cannot be a floor.
        assert!(
            error(quote! { name = "p"; version = "2.0"; min = "1.0"; })
                .contains("must share a major")
        );
        // Requiring a peer newer than yourself refuses every peer, silently.
        assert!(
            error(quote! { name = "p"; version = "1.0"; min = "1.5"; })
                .contains("must not be newer")
        );
    }

    #[test]
    fn rejects_a_duplicated_method() {
        let message = error(quote! {
            name = "p"; version = "1.0";
            initiator fn echo(A) -> B;
            responder fn "echo" echo_again(A) -> B;
        });
        assert!(message.contains("declared twice"), "{message}");
    }

    #[test]
    fn the_handshake_may_be_declared_by_a_protocol_that_owns_it() {
        // mira answers `initialize` with its eval catalogue, MCP with
        // serverInfo and a nested capabilities object. Neither is lanok's
        // Hello, and neither should have to become one to be typed.
        let protocol = parse(quote! {
            name = "p"; version = "1.0";
            initiator fn initialize(InitializeParams) -> InitializeResult;
            initiator notify "notifications/initialized" initialized();
        })
        .unwrap();
        assert_eq!(protocol.methods[0].wire_name, "initialize");
        assert_eq!(protocol.methods[1].wire_name, "notifications/initialized");
    }

    #[test]
    fn rejects_a_capability_that_was_never_declared() {
        // A typo here fails closed at runtime: the method silently becomes
        // unavailable forever. Catching it at compile time is the whole point.
        let message = error(quote! {
            name = "p"; version = "1.0";
            initiator fn stream(A) -> B requires "streming";
            capabilities { streaming }
        });
        assert!(message.contains("streming"), "{message}");
        assert!(message.contains("capabilities"), "{message}");
    }

    #[test]
    fn rejects_a_notification_with_a_result() {
        let message = error(quote! {
            name = "p"; version = "1.0";
            initiator notify tick(A) -> B;
        });
        assert!(message.contains("carries no id"), "{message}");
    }

    #[test]
    fn rejects_an_unknown_direction_or_kind() {
        assert!(
            error(quote! { name = "p"; version = "1.0"; sideways fn a(); })
                .contains("`initiator` or `responder`")
        );
        assert!(
            error(quote! { name = "p"; version = "1.0"; initiator shout a(); })
                .contains("`fn` or `notify`")
        );
    }

    #[test]
    fn rejects_an_unknown_setting() {
        assert!(error(quote! { name = "p"; versoin = "1.0"; }).contains("unknown setting"));
    }
}
