//! Code generation for a parsed `protocol!` declaration.
//!
//! Four kinds of item come out, and the split is the whole design:
//!
//! * **Vocabulary** (`META`, `method::*`, `NEGOTIATION`) so the protocol
//!   describes itself as data for artifacts and tools.
//! * **Stubs**, one trait per direction, implemented for `Peer`. Role gating is
//!   by trait: importing the wrong one does not compile, so a client cannot
//!   accidentally call a server-only method.
//! * **Handler traits**, one per direction, every method defaulting to
//!   `method not found`. Implement only what you answer.
//! * **Dispatchers** that adapt a handler trait to `lanok::Handler`, doing the
//!   deserialize / call / serialize dance once instead of per method per crate.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

use crate::parse::{Direction, Method, Protocol};

pub fn expand(protocol: Protocol) -> TokenStream {
    let Protocol {
        name,
        version,
        min_version,
        capabilities,
        methods,
    } = protocol;

    let (major, minor) = version;
    let (min_major, min_minor) = min_version;

    let method_consts = methods.iter().map(|m| {
        let ident = format_ident!("{}", m.ident.to_string().to_uppercase());
        let wire = &m.wire_name;
        let doc = format!("`{}`", m.wire_name);
        quote! {
            #[doc = #doc]
            pub const #ident: &str = #wire;
        }
    });

    let method_metas = methods.iter().map(|m| {
        let wire = &m.wire_name;
        let direction = m.direction.meta_path();
        let kind = if m.expects_response {
            quote!(::lanok::MethodKind::Request)
        } else {
            quote!(::lanok::MethodKind::Notification)
        };
        let doc = &m.doc;
        let requires = match &m.requires {
            Some(token) => quote!(Some(#token)),
            None => quote!(None),
        };
        // The payload type's *name*, so `meta.json` says what each method
        // carries and an SDK generator can emit a typed method instead of a
        // string constant. The shape itself is `schema.json`'s job; this is the
        // key into it. Taken from the declaration's last path segment, so
        // `crate::wire::RunParams` is published as `RunParams` — which is what
        // the schema's `$defs` are keyed by.
        let params = type_name(m.params.as_ref());
        let result = type_name(m.result.as_ref());
        quote! {
            ::lanok::MethodMeta {
                name: #wire,
                direction: #direction,
                kind: #kind,
                doc: #doc,
                requires: #requires,
                params: #params,
                result: #result,
            }
        }
    });

    // A token's doc is the protocol's own prose about what advertising it
    // promises. Generating a placeholder over the top of it would make moving
    // tokens into `protocol!` a downgrade, so the declaration's doc wins and
    // the placeholder is only the fallback.
    let capability_consts = capabilities.iter().map(|capability| {
        let token = &capability.token;
        let ident = format_ident!("{}", token.to_uppercase());
        let doc = if capability.doc.is_empty() {
            format!("The `{token}` capability token.")
        } else {
            capability.doc.clone()
        };
        quote! {
            #[doc = #doc]
            pub const #ident: &str = #token;
        }
    });
    let capability_list = capabilities.iter().map(|c| {
        let token = &c.token;
        quote!(#token)
    });

    let initiator_stubs = stubs(&methods, Direction::Initiator);
    let responder_stubs = stubs(&methods, Direction::Responder);
    let shared_stubs = stubs(&methods, Direction::Either);
    // A handler answers what the *other* side sends.
    let responder_handlers = handler_trait(&methods, Direction::Initiator, "ResponderHandler");
    let initiator_handlers = handler_trait(&methods, Direction::Responder, "InitiatorHandler");
    let responder_dispatch = dispatch(
        &methods,
        Direction::Initiator,
        "ResponderHandler",
        "ResponderDispatch",
    );
    let initiator_dispatch = dispatch(
        &methods,
        Direction::Responder,
        "InitiatorHandler",
        "InitiatorDispatch",
    );

    let initiator_api = api_trait(&methods, Direction::Initiator, "InitiatorApi");
    let responder_api = api_trait(&methods, Direction::Responder, "ResponderApi");
    let shared_api = api_trait(&methods, Direction::Either, "SharedApi");
    let schema_document = schema_document(&methods);

    let name_doc = format!("The `{name}` protocol, version {major}.{minor}.");

    quote! {
        #[doc = #name_doc]
        pub const PROTOCOL_NAME: &str = #name;

        /// The protocol version this build implements.
        pub const PROTOCOL_VERSION: ::lanok::Version = ::lanok::Version::new(#major, #minor);

        /// The oldest peer version this build accepts.
        pub const MIN_PROTOCOL_VERSION: ::lanok::Version =
            ::lanok::Version::new(#min_major, #min_minor);

        /// What to hand [`lanok::Peer::handshake`].
        pub const NEGOTIATION: ::lanok::Negotiation =
            ::lanok::Negotiation::with_min(PROTOCOL_VERSION, MIN_PROTOCOL_VERSION);

        /// The protocol's full vocabulary, as data. Serialized to `meta.json`.
        pub const META: ::lanok::ProtocolMeta = ::lanok::ProtocolMeta {
            name: #name,
            version: PROTOCOL_VERSION,
            min_version: MIN_PROTOCOL_VERSION,
            methods: &[ #(#method_metas),* ],
            capabilities: &[ #(#capability_list),* ],
        };

        /// Wire names, so a call site never spells a method as a string.
        pub mod method {
            #(#method_consts)*
        }

        /// Capability tokens this protocol defines.
        pub mod capability {
            #(#capability_consts)*
        }

        #initiator_api
        #responder_api
        #shared_api
        #initiator_stubs
        #responder_stubs
        #shared_stubs
        #responder_handlers
        #initiator_handlers
        #responder_dispatch
        #initiator_dispatch
        #schema_document
    }
}

/// The last path segment of a declared payload type, as a string literal, or
/// `None`. `schemars` keys `$defs` by the bare type name, so publishing the
/// full path would hand an SDK generator a key that is not in the schema.
fn type_name(ty: Option<&syn::Type>) -> TokenStream {
    let Some(syn::Type::Path(path)) = ty else {
        return quote!(None);
    };
    match path.path.segments.last() {
        Some(segment) => {
            let name = segment.ident.to_string();
            quote!(Some(#name))
        }
        None => quote!(None),
    }
}

/// The generated schema builder.
///
/// Emitting the calls from the same declaration that emits the stubs is what
/// makes drift impossible rather than merely discouraged: a method cannot be
/// added to the protocol without appearing in the artifact.
///
/// It sits behind the *protocol crate's* own `schema` feature, so a crate that
/// only wants wire types never compiles schemars. That crate declares
/// `schema = ["lanok/schema", ...]`; the builder is reached through the facade,
/// so there is no second dependency to keep version-matched.
fn schema_document(methods: &[Method]) -> TokenStream {
    let calls = methods.iter().map(|m| {
        let wire = &m.wire_name;
        match (&m.params, &m.result, m.expects_response) {
            (Some(params), Some(result), true) => {
                quote!(.request::<#params, #result>(#wire))
            }
            (Some(params), None, true) => quote!(.request_params::<#params>(#wire)),
            (None, Some(result), true) => quote!(.request_result::<#result>(#wire)),
            (None, None, true) => quote!(.request_bare(#wire)),
            (Some(params), _, false) => quote!(.notification::<#params>(#wire)),
            (None, _, false) => quote!(.notification_bare(#wire)),
        }
    });

    quote! {
        /// The protocol's schema and vocabulary artifacts, generated from this
        /// declaration. Feed it to `lanok_schema::Artifacts`.
        #[cfg(feature = "schema")]
        pub fn schema_document() -> ::lanok::schema::Document {
            ::lanok::schema::Document::new(META)
                #(#calls)*
        }
    }
}

/// The trait declaration for one direction's outbound calls.
fn api_trait(methods: &[Method], direction: Direction, trait_name: &str) -> TokenStream {
    let trait_ident = format_ident!("{}", trait_name);
    let doc = match direction {
        Direction::Either => "Methods **either side** may send. Implemented for \
             [`lanok::Peer`].\n\nThese are on their own trait rather than on both role \
             traits, so importing both roles cannot make a call ambiguous. They carry no \
             role gating, which is the cost of declaring a method `either`."
            .to_string(),
        role => {
            let role = match role {
                Direction::Initiator => "initiator",
                _ => "responder",
            };
            format!(
                "Methods the {role} sends. Implemented for [`lanok::Peer`]; import it to call \
                 them.\n\nRole gating is by trait: the other side's methods are not on this \
                 one, so calling a method in the wrong direction does not compile."
            )
        }
    };

    let signatures = methods
        .iter()
        .filter(|m| m.direction == direction)
        .map(|m| {
            let ident = &m.ident;
            let doc = if m.doc.is_empty() {
                format!("`{}`", m.wire_name)
            } else {
                format!("{}\n\nWire name: `{}`.", m.doc, m.wire_name)
            };
            let params = param_arg(m);
            if m.expects_response {
                let result = result_type(m);
                quote! {
                    #[doc = #doc]
                    async fn #ident(&self #params) -> ::core::result::Result<#result, ::lanok::RpcError>;
                }
            } else {
                quote! {
                    #[doc = #doc]
                    fn #ident(&self #params);
                }
            }
        });

    quote! {
        #[doc = #doc]
        #[::lanok::async_trait]
        pub trait #trait_ident {
            #(#signatures)*
        }
    }
}

/// `impl <Api> for Peer`, where the bodies live.
fn stubs(methods: &[Method], direction: Direction) -> TokenStream {
    let trait_ident = match direction {
        Direction::Initiator => format_ident!("InitiatorApi"),
        Direction::Responder => format_ident!("ResponderApi"),
        Direction::Either => format_ident!("SharedApi"),
    };

    let bodies = methods
        .iter()
        .filter(|m| m.direction == direction)
        .map(|m| {
            let ident = &m.ident;
            let wire = &m.wire_name;
            let params = param_arg(m);
            let to_value = match &m.params {
                Some(_) => quote! {
                    match ::lanok::to_value(&params) {
                        Ok(value) => value,
                        Err(e) => return Err(::lanok::RpcError::invalid_params(
                            ::std::format!("params are not serializable: {e}")
                        )),
                    }
                },
                None => quote!(::lanok::Value::Null),
            };

            // Capability gating happens before the wire, so an unsupported
            // method is a typed local answer instead of a round trip that ends
            // in `method not found`.
            let gate = match &m.requires {
                Some(token) => quote! {
                    if !::lanok::Peer::supports(self, #token) {
                        return Err(::lanok::RpcError::capability_unsupported(#wire, #token));
                    }
                },
                None => quote!(),
            };

            if m.expects_response {
                let result = result_type(m);
                let decode = match &m.result {
                    Some(_) => quote! {
                        ::lanok::from_value(raw).map_err(|e| ::lanok::RpcError::internal(
                            ::std::format!("malformed `{}` result: {e}", #wire)
                        ))
                    },
                    None => quote!(Ok(())),
                };
                quote! {
                    async fn #ident(&self #params) -> ::core::result::Result<#result, ::lanok::RpcError> {
                        #gate
                        let params = #to_value;
                        let raw = ::lanok::Peer::request(self, #wire, params).await?;
                        #[allow(clippy::let_unit_value)]
                        { let _ = &raw; }
                        #decode
                    }
                }
            } else {
                // A notification has nobody to report a failure to: no id, no
                // response. So both the capability check and a serialization
                // failure drop the send rather than surfacing an error that
                // would have to go somewhere. Encoding is therefore its own
                // fragment, not the request path's (which returns Err).
                let gate = match &m.requires {
                    Some(token) => quote! {
                        if !::lanok::Peer::supports(self, #token) { return; }
                    },
                    None => quote!(),
                };
                let encode = match &m.params {
                    Some(_) => quote! {
                        let Ok(params) = ::lanok::to_value(&params) else { return };
                    },
                    None => quote!(let params = ::lanok::Value::Null;),
                };
                quote! {
                    fn #ident(&self #params) {
                        #gate
                        #encode
                        ::lanok::Peer::notify(self, #wire, params);
                    }
                }
            }
        });

    quote! {
        #[::lanok::async_trait]
        impl #trait_ident for ::lanok::Peer {
            #(#bodies)*
        }
    }
}

/// The trait a side implements to answer what the other side sends.
fn handler_trait(methods: &[Method], incoming: Direction, trait_name: &str) -> TokenStream {
    let trait_ident = format_ident!("{}", trait_name);
    let answering = match incoming {
        Direction::Initiator => "responder",
        _ => "initiator",
    };
    let doc = format!(
        "What the {answering} answers.\n\n\
         Every method defaults to `method not found`, so implement only what you actually \
         handle. A method left unimplemented refuses politely rather than hanging the caller \
         until its timeout."
    );

    let signatures = methods.iter().filter(|m| answers(m, incoming)).map(|m| {
        let ident = &m.ident;
        let wire = &m.wire_name;
        let doc = if m.doc.is_empty() {
            format!("Answers `{}`.", m.wire_name)
        } else {
            format!("{}\n\nAnswers `{}`.", m.doc, m.wire_name)
        };
        let params = param_arg(m);
        let unused = match &m.params {
            Some(_) => quote!(let _ = (cx, params);),
            None => quote!(let _ = cx;),
        };
        // Every method takes the context, including the ones that ignore it.
        // A handler that later needs the request id or wants to stream
        // progress then changes its body, not the protocol's shape, and the
        // two serve loops stay interchangeable.
        if m.expects_response {
            let result = result_type(m);
            quote! {
                #[doc = #doc]
                async fn #ident(&self, cx: ::lanok::Context #params) -> ::core::result::Result<#result, ::lanok::RpcError> {
                    #unused
                    Err(::lanok::RpcError::method_not_found(#wire))
                }
            }
        } else {
            quote! {
                #[doc = #doc]
                fn #ident(&self, cx: ::lanok::Context #params) {
                    #unused
                }
            }
        }
    });

    quote! {
        #[doc = #doc]
        #[::lanok::async_trait]
        pub trait #trait_ident: Send + Sync + 'static {
            #(#signatures)*
        }
    }
}

/// The adapter from a handler trait to `lanok::Handler`.
fn dispatch(
    methods: &[Method],
    incoming: Direction,
    trait_name: &str,
    struct_name: &str,
) -> TokenStream {
    let trait_ident = format_ident!("{}", trait_name);
    let struct_ident = format_ident!("{}", struct_name);

    let request_arms = methods
        .iter()
        .filter(|m| answers(m, incoming) && m.expects_response)
        .map(request_arm);
    let notification_arms = methods
        .iter()
        .filter(|m| answers(m, incoming) && !m.expects_response)
        .map(notification_arm);

    let doc = format!(
        "Adapts a [`{trait_name}`] into a [`lanok::Handler`], so it can be installed on a peer.\n\n\
         The dispatcher is exhaustive over the declaration: adding a method to the `protocol!` \
         block routes it here automatically, and an unknown method answers `method not found`."
    );

    quote! {
        #[doc = #doc]
        #[derive(Debug)]
        pub struct #struct_ident<H>(::std::sync::Arc<H>);

        impl<H: #trait_ident> #struct_ident<H> {
            pub fn new(handler: H) -> Self {
                Self(::std::sync::Arc::new(handler))
            }

            /// Wrap a handler the caller already shares.
            pub fn shared(handler: ::std::sync::Arc<H>) -> Self {
                Self(handler)
            }
        }

        impl<H: #trait_ident> ::lanok::Handler for #struct_ident<H> {
            fn request(&self, cx: ::lanok::Context, method: ::std::string::String, params: ::lanok::Value)
                -> ::lanok::HandlerFuture
            {
                // The handler is behind an Arc so the returned future owns what
                // it needs: Handler::request hands back a 'static future and
                // cannot borrow self.
                let handler = self.0.clone();
                ::std::boxed::Box::pin(async move {
                    match method.as_str() {
                        #(#request_arms)*
                        other => Err(::lanok::RpcError::method_not_found(other)),
                    }
                })
            }

            fn notification(&self, cx: ::lanok::Context, method: ::std::string::String, params: ::lanok::Value) {
                match method.as_str() {
                    #(#notification_arms)*
                    _ => {}
                }
            }
        }
    }
}

/// Whether a handler for messages arriving from `incoming` answers this method.
///
/// A method either side may send arrives from both, so both handler traits
/// carry it and both dispatchers route it.
fn answers(method: &Method, incoming: Direction) -> bool {
    method.direction == incoming || method.direction == Direction::Either
}

fn request_arm(method: &Method) -> TokenStream {
    let ident = &method.ident;
    let wire = &method.wire_name;
    let decode = decode_params(method);
    let call = match &method.params {
        Some(_) => quote!(handler.#ident(cx, params).await),
        None => quote!(handler.#ident(cx).await),
    };
    let encode = match &method.result {
        Some(_) => quote! {
            ::lanok::to_value(&value).map_err(|e| ::lanok::RpcError::internal(
                ::std::format!("`{}` result is not serializable: {e}", #wire)
            ))
        },
        None => quote!(Ok(::lanok::Value::Null)),
    };
    quote! {
        #wire => {
            #decode
            let value = #call?;
            #[allow(clippy::let_unit_value)]
            { let _ = &value; }
            #encode
        }
    }
}

fn notification_arm(method: &Method) -> TokenStream {
    let ident = &method.ident;
    let wire = &method.wire_name;
    match &method.params {
        // A notification has nobody to report a decode failure to, so a
        // malformed one is dropped rather than pretended to be handled.
        Some(ty) => quote! {
            #wire => {
                if let Ok(params) = ::lanok::from_value::<#ty>(params) {
                    self.0.#ident(cx, params);
                }
            }
        },
        None => quote! {
            #wire => self.0.#ident(cx),
        },
    }
}

fn decode_params(method: &Method) -> TokenStream {
    match &method.params {
        Some(ty) => {
            let wire = &method.wire_name;
            quote! {
                let params: #ty = ::lanok::from_value(params).map_err(|e| {
                    ::lanok::RpcError::invalid_params(
                        ::std::format!("`{}` params are malformed: {e}", #wire)
                    )
                })?;
            }
        }
        None => quote!(let _ = params;),
    }
}

fn param_arg(method: &Method) -> TokenStream {
    match &method.params {
        Some(ty) => quote!(, params: #ty),
        None => quote!(),
    }
}

fn result_type(method: &Method) -> TokenStream {
    match &method.result {
        Some(ty) => quote!(#ty),
        None => quote!(()),
    }
}

#[allow(dead_code)]
fn unused(_: &Ident) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expanding is the only place a capability's doc becomes observable: doc
    /// comments do not survive to runtime, so a test on the generated tokens
    /// is what keeps the declaration's prose from being replaced by a
    /// generated one-liner.
    #[test]
    fn a_capability_const_carries_the_declared_doc() {
        let declaration: crate::parse::Protocol = syn::parse_quote! {
            name    = "p";
            version = "1.0";

            initiator fn go() requires "streaming";

            capabilities {
                /// The peer streams partial results while a call is open.
                streaming,
            }
        };
        let expanded = expand(declaration).to_string();
        assert!(
            expanded.contains("The peer streams partial results while a call is open."),
            "the declared doc should reach the generated const"
        );
    }

    /// Without a doc, the placeholder still describes the token, so an
    /// undocumented declaration does not become an undocumented public const.
    #[test]
    fn an_undocumented_capability_still_gets_a_doc() {
        let declaration: crate::parse::Protocol = syn::parse_quote! {
            name    = "p";
            version = "1.0";

            initiator fn go() requires "streaming";

            capabilities { streaming }
        };
        let expanded = expand(declaration).to_string();
        assert!(expanded.contains("The `streaming` capability token."));
    }

    #[test]
    fn a_capability_declared_twice_is_rejected_on_the_declaration() {
        let error = match syn::parse_str::<crate::parse::Protocol>(
            r#"name = "p"; version = "1.0"; capabilities { a, a }"#,
        ) {
            Err(error) => error,
            Ok(_) => panic!("a duplicate token must not parse"),
        };
        assert!(
            error.to_string().contains("declared twice"),
            "unexpected error: {error}"
        );
    }
}
