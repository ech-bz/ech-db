use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Data, DeriveInput, Fields, Path};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let binding_crate: Path = parse_quote!(::ech_db_guest);
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

    let cleanup = fields.named.iter().map(|field| {
        let ident = field.ident.as_ref().expect("named field");
        quote! {
            #binding_crate::Bind::cleanup(&self.#ident)?;
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

            fn cleanup(&self) -> ::std::result::Result<(), #binding_crate::GuestError> {
                #(#cleanup)*
                Ok(())
            }
        }
    })
}
