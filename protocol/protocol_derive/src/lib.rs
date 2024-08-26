use attribute_derive::FromAttr;
use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{parse_macro_input, DeriveInput, Meta};

#[derive(FromAttr)]
#[attribute(ident = protocol)]
struct Protocol {
    output: syn::Type,
    #[attribute(optional)]
    args_name: Vec<syn::Ident>,
}

#[proc_macro_derive(Protocol, attributes(protocol))]
pub fn protocol_derive(input: TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    if let syn::Data::Enum(data) = &input.data {
        let ty = &input.ident;
        let arms = data.variants.iter().map(|variant| {
            let ident = &variant.ident;
            let attr: Protocol =
                Protocol::from_attributes(&variant.attrs).expect("Could not parse attr");
            let name: syn::Ident = format_ident!("{}", &ident.to_string().to_case(Case::Snake));
            let output = attr.output;
            let args_name = &attr.args_name;
            if args_name.len() != variant.fields.len() {
                return quote!{compile_error!("args_name et variant have a different number of fields");};
            }
            let args: Vec<_> = args_name
                .iter()
                .zip(variant.fields.iter())
                .map(|(name, ty)| quote! { #name: #ty })
                .collect();
            let mut out_tokens = output.to_token_stream().to_string();
            out_tokens.retain(|c| c != ' ');
            let doc = format!("Reponse type is `{out_tokens}`");
            if args.is_empty() {
                quote! {
                    #[doc = #doc]
                    pub fn #name() -> (Action, Receiver<DataType>) {
                        Action::new(#ty::#ident)
                    }

                }
            } else {
                quote! {
                    #[doc = #doc]
                    pub fn #name(#(#args),*) -> (Action, Receiver<DataType>) {
                        Action::new(#ty::#ident(#(#args_name),*))
                    }
                }
            }
        });
        quote! {
            impl #ty {
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
