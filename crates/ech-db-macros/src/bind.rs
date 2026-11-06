use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Data, DeriveInput, Fields, LitStr, Path};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let mut binding_crate: Path = parse_quote!(::ech_db_guest);
    for attr in &input.attrs {
        if attr.path().is_ident("bind") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("crate") {
                    binding_crate = meta.value()?.parse::<LitStr>()?.parse()?;
                    Ok(())
                } else {
                    Err(meta.error("unsupported Bind attribute"))
                }
            })?;
        }
    }
    let name = input.ident;
    let mut generics = input.generics;
    let Data::Struct(data) = input.data else {
        return Err(syn::Error::new_spanned(&name, "`Bind` requires a struct"));
    };
    let Fields::Named(fields) = data.fields else {
        return Err(syn::Error::new_spanned(
            &name,
            "`Bind` requires named fields",
        ));
    };
    for field in &fields.named {
        let ty = &field.ty;
        generics.make_where_clause().predicates.push(parse_quote!(
            #ty: #binding_crate::Bind
        ));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let body = fields.named.iter().map(|field| {
        let ident = field.ident.as_ref().expect("named field");
        let text = ident.to_string();
        quote! {
            #binding_crate::Bind::bind(&mut self.#ident, &(name, #text));
        }
    });

    Ok(quote! {
        impl #impl_generics #binding_crate::Bind for #name #ty_generics #where_clause {
            fn bind<__EchBindName: #binding_crate::serde::Serialize + ?Sized>(
                &mut self,
                name: &__EchBindName,
            ) {
                #(#body)*
            }
        }
    })
}
