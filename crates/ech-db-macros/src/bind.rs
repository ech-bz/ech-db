use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Data, DeriveInput, Fields, Path};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let binding_crate: Path = parse_quote!(::ech_db_guest);
    let name = input.ident;
    let mut generics = input.generics;
    let data = input.data;

    let mut types: Vec<&syn::Type> = Vec::new();
    let bind;
    let cleanup;
    match &data {
        Data::Struct(data) => {
            let Fields::Named(fields) = &data.fields else {
                return Err(syn::Error::new_spanned(
                    &name,
                    "`Bind` requires named fields",
                ));
            };
            for field in &fields.named {
                types.push(&field.ty);
            }
            let body = fields.named.iter().map(|field| {
                let ident = field.ident.as_ref().expect("named field");
                let text = ident.to_string();
                quote! {
                    #binding_crate::Bind::bind(&mut self.#ident, &(name, #text));
                }
            });
            let cleanups = fields.named.iter().map(|field| {
                let ident = field.ident.as_ref().expect("named field");
                quote! {
                    #binding_crate::Bind::cleanup(&self.#ident)?;
                }
            });
            bind = quote! { #(#body)* };
            cleanup = quote! {
                #(#cleanups)*
                ::core::result::Result::Ok(())
            };
        }
        Data::Enum(data) => {
            if data.variants.is_empty() {
                return Err(syn::Error::new_spanned(
                    &name,
                    "`Bind` requires at least one variant",
                ));
            }
            for variant in &data.variants {
                for field in &variant.fields {
                    types.push(&field.ty);
                }
            }
            let binds = data.variants.iter().map(|variant| {
                let ident = &variant.ident;
                match &variant.fields {
                    Fields::Unit => quote! { Self::#ident => {} },
                    Fields::Named(fields) => {
                        let names: Vec<_> = fields
                            .named
                            .iter()
                            .map(|field| field.ident.as_ref().expect("named field").clone())
                            .collect();
                        let body = names.iter().map(|field| {
                            let text = field.to_string();
                            quote! {
                                #binding_crate::Bind::bind(#field, &(name, #text));
                            }
                        });
                        quote! {
                            Self::#ident { #(#names),* } => {
                                #(#body)*
                            }
                        }
                    }
                    Fields::Unnamed(fields) => {
                        let names: Vec<_> = (0..fields.unnamed.len())
                            .map(|index| format_ident!("__ech_value_{index}"))
                            .collect();
                        let body = names.iter().enumerate().map(|(index, field)| {
                            let text = index.to_string();
                            quote! {
                                #binding_crate::Bind::bind(#field, &(name, #text));
                            }
                        });
                        quote! {
                            Self::#ident(#(#names),*) => {
                                #(#body)*
                            }
                        }
                    }
                }
            });
            let cleanups = data.variants.iter().map(|variant| {
                let ident = &variant.ident;
                match &variant.fields {
                    Fields::Unit => quote! { Self::#ident => {} },
                    Fields::Named(fields) => {
                        let names: Vec<_> = fields
                            .named
                            .iter()
                            .map(|field| field.ident.as_ref().expect("named field").clone())
                            .collect();
                        let body = names.iter().map(|field| {
                            quote! {
                                #binding_crate::Bind::cleanup(#field)?;
                            }
                        });
                        quote! {
                            Self::#ident { #(#names),* } => {
                                #(#body)*
                            }
                        }
                    }
                    Fields::Unnamed(fields) => {
                        let names: Vec<_> = (0..fields.unnamed.len())
                            .map(|index| format_ident!("__ech_value_{index}"))
                            .collect();
                        let body = names.iter().map(|field| {
                            quote! {
                                #binding_crate::Bind::cleanup(#field)?;
                            }
                        });
                        quote! {
                            Self::#ident(#(#names),*) => {
                                #(#body)*
                            }
                        }
                    }
                }
            });
            bind = quote! {
                match self {
                    #(#binds)*
                }
            };
            cleanup = quote! {
                match self {
                    #(#cleanups)*
                }
                ::core::result::Result::Ok(())
            };
        }
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                &name,
                "`Bind` requires a struct or an enum",
            ));
        }
    }

    for ty in types {
        generics.make_where_clause().predicates.push(parse_quote!(
            #ty: #binding_crate::Bind
        ));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #binding_crate::Bind for #name #ty_generics #where_clause {
            #[allow(unused_variables)]
            fn bind<__EchBindName: #binding_crate::serde::Serialize + ?Sized>(
                &mut self,
                name: &__EchBindName,
            ) {
                #bind
            }

            fn cleanup(&self) -> ::std::result::Result<(), #binding_crate::GuestError> {
                #cleanup
            }
        }
    })
}
