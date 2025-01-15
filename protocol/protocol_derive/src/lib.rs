use attribute_derive::FromAttr;
use heck::AsSnakeCase;
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{parse_macro_input, DeriveInput, Ident, Variant};

#[derive(FromAttr, Debug)]
#[attribute(ident = protocol)]
struct Protocol {
    output: syn::Type,
    #[attribute(optional)]
    args_name: Vec<syn::Ident>,
}

#[derive(FromAttr)]
#[attribute(ident = skip)]
struct Skip {}

fn parse_arms(variant: &Variant, ty: &Ident) -> impl ToTokens {
    if let Ok(attr) = Protocol::from_attributes(&variant.attrs) {
        let ident = &variant.ident;
        let name: syn::Ident = format_ident!("{}", format!("{}", AsSnakeCase(ident.to_string())));
        let output = attr.output;
        let args_name = &attr.args_name;
        if args_name.len() != variant.fields.len() {
            return quote! {compile_error!("args_name et variant have a different number of fields");};
        }
        let args: Vec<_> = args_name
            .iter()
            .zip(variant.fields.iter())
            .map(|(name, ty)| quote! { #name: #ty })
            .collect();
        let mut out_tokens = output.to_token_stream().to_string();
        out_tokens.retain(|c| c != ' ');
        let doc = format!("Reponse type is `Result<{out_tokens}>`");
        let variant_args = if args.is_empty() {
            quote! {}
        } else {
            quote! {(#(#args_name),*)}
        };
        quote! {
            #[doc = #doc]
            pub fn #name(#(#args),*) -> TypedAction<Result<#output>> {
                TypedAction::from_command(#ty::#ident #variant_args)
            }

        }
    } else {
        for attribute in &variant.attrs {
            if attribute.path().is_ident("skip") {
                return quote! {};
            }
        }
        quote! {}
    }
}

#[proc_macro_derive(Protocol, attributes(protocol, skip))]
pub fn protocol_derive(input: TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if let syn::Data::Enum(data) = &input.data {
        let ty = &input.ident;
        let (impl_gen, ty_gen, where_clause) = &input.generics.split_for_impl();
        let arms = data.variants.iter().map(|variant| parse_arms(variant, ty));
        quote! {
            #[automatically_derived]
            impl #impl_gen #ty #ty_gen #where_clause {
                #(#arms)*
            }
        }
        .into()
    } else {
        quote! {
            compile_error!("Only `enum` are supported");
        }
        .into()
    }
}
