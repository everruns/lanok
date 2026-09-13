//! The `protocol!` declaration macro. Use it through the `lanok` facade crate,
//! which is what the generated code refers to.

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod expand;
mod parse;

/// Declare a JSON-RPC protocol: its methods, their directions, and the
/// capabilities they need.
///
/// See [`lanok::protocol`](../lanok/macro.protocol.html) for the full grammar
/// and what it generates.
#[proc_macro]
pub fn protocol(input: TokenStream) -> TokenStream {
    let declaration = parse_macro_input!(input as parse::Protocol);
    expand::expand(declaration).into()
}
