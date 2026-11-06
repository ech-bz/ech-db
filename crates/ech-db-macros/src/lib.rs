mod bind;

use proc_macro::TokenStream;

#[proc_macro_derive(Bind, attributes(bind))]
pub fn bind(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    bind::expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
