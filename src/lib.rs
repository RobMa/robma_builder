//! My result of the builder exercise in the excellent [proc-macro-workshop](https://github.com/dtolnay/proc-macro-workshop) by David Tolnay.
//!
//! I strongly recommend checking out this workshop to anyone who would like to get started with procedural macros in rust.
//!
//! This is a personal learning project, do **not** use in production and consider [rust-derive-builder](https://crates.io/crates/derive_builder) instead.

extern crate proc_macro;
extern crate quote;
extern crate syn;

use quote::{format_ident, quote};

/// Derives a `builder()` method that can be used to construct an instance of a `struct`.
///
/// The library is implemented using a procedural macro.
/// The builder is a fluent-design pattern that simplifies the construction of a struct instance.
/// In particular, we support the following features
/// - No uninitialized members: checks the presence of all variables and panics in case of missing variables.
/// - Members of type `Option` do not have to be specified and default to `None`
/// - Repeated arguments: Members of type `Vec` can be constructed using a sequence of individual elements.
///
/// # Examples:
/// ## Basic usage
/// ```
/// use robma_builder::Builder;
///
/// #[derive(Builder, Debug, PartialEq)]
/// pub struct Command {
///     executable: String,
///     args: Vec<String>,
///     env: Vec<String>,
///     current_dir: String,
/// }
///
/// let command = Command::builder()
///        .executable("cargo".into())
///        .args(vec!["build".into(), "--release".into()])
///        .env(vec![])
///        .current_dir("..".into())
///        .build()
///        .expect("missing arguments");
///
/// // The result of the above is equivalent to:
/// let command_equivalent = Command{
///         executable: "cargo".into(),
///         args: vec!["build".into(), "--release".into()],
///         env: vec![],
///         current_dir: "..".into(),
/// };
/// assert_eq!(command, command_equivalent);
/// ```
///
/// ## Optional members
/// Optional members can be ommitted in the `builder` and initialized with `None`
/// ```
/// use robma_builder::Builder;
///
/// #[derive(Builder)]
/// pub struct Command {
///     current_dir: Option<String>,
/// }
///
/// let command = Command::builder()
///     .build()
///     .expect("missing arguments");
///
/// // Unspecified option members will be initialized with None:
/// assert!(command.current_dir.is_none());
/// ```
///
/// ## Repeated arguments
/// Members of type `Vec` can be initialized by a sequence of elements.
/// Use the attribute `#[builder(each = "...")]` to declare the name of the function.
/// ```
/// use robma_builder::Builder;
///
/// #[derive(Builder)]
/// pub struct Command {
///     #[builder(each = "arg")]
///     args: Vec<String>,
/// }
///
/// let command = Command::builder()
///     .arg("build".to_owned())
///     .arg("--release".to_owned())
///     .build()
///     .expect("missing arguments");
///
/// // args contains the elements of each call to `arg()`
/// assert_eq!(command.args, vec!["build", "--release"]);
/// ```
#[proc_macro_derive(Builder, attributes(builder))]
pub fn derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let derive_input: syn::DeriveInput = syn::parse_macro_input!(input);

    let name = &derive_input.ident;
    let builder_name = format_ident!("{}Builder", name);
    let fields = get_fields(&derive_input);

    if let Some(error_messages) = check_fields_for_errors(&fields) {
        return error_messages.into();
    }

    let struct_body = fields
        .iter()
        .map(|x| {
            let name = x.name;
            let field_type = x.field_type;
            if x.repeated_name
                .as_ref()
                .expect("Unexpected repeated_name error")
                .is_none()
            {
                quote! {
                    #name: std::option::Option<#field_type>
                }
            } else {
                quote! {
                    #name: #field_type
                }
            }
        })
        .collect::<Vec<proc_macro2::TokenStream>>();

    let builder_body = fields
        .iter()
        .map(|x| {
            let name = x.name;
            let initial_value = if x
                .repeated_name
                .as_ref()
                .expect("Unexpected repeated_name error")
                .is_some()
            {
                quote! {vec![]}
            } else {
                quote! {None}
            };
            quote! {
                #name: #initial_value
            }
        })
        .collect::<Vec<proc_macro2::TokenStream>>();

    let setter_functions = derive_setter_functions(&fields);

    let build_function = derive_build_function(name, &fields);

    let out = quote! {
        #[derive(Debug, PartialEq)]
        struct #builder_name{
            #(#struct_body),*
        }

        impl #name {
            fn builder() -> #builder_name {
                #builder_name {
                    #(#builder_body),*
                }
            }
        }

        impl #builder_name {
            #setter_functions

            #build_function
        }

    };

    out.into()
}

struct Field<'f> {
    name: &'f syn::Ident,
    field_type: &'f syn::Type,
    optional: bool,
    repeated_name: Result<Option<syn::Ident>, syn::Error>,
}

fn get_fields<'f>(derive_input: &'f syn::DeriveInput) -> Vec<Field<'f>> {
    let fields = {
        if let syn::Data::Struct(data_struct) = &derive_input.data {
            if let syn::Fields::Named(fields) = &data_struct.fields {
                &fields.named
            } else {
                unimplemented!()
            }
        } else {
            unimplemented!()
        }
    };

    fields
        .iter()
        .map(|x| {
            if is_option(&x.ty) {
                Field {
                    name: x.ident.as_ref().expect("Expected identifier"),
                    field_type: get_angle_bracket_arg(&x.ty).expect("Expected Option Type"),
                    optional: true,
                    repeated_name: get_repeated_name(&x.attrs),
                }
            } else {
                Field {
                    name: x.ident.as_ref().expect("Expected identifier"),
                    field_type: &x.ty,
                    optional: false,
                    repeated_name: get_repeated_name(&x.attrs),
                }
            }
        })
        .collect()
}

fn check_fields_for_errors(fields: &[Field]) -> Option<proc_macro2::TokenStream> {
    let error_messages: Vec<proc_macro2::TokenStream> = fields
        .iter()
        .filter(|field| field.repeated_name.is_err())
        .map(|field| {
            field
                .repeated_name
                .as_ref()
                .expect_err("Expected repeated_name error")
                .to_compile_error()
        })
        .collect();
    if error_messages.is_empty() {
        None
    } else {
        Some(quote! {
            #(#error_messages)*
        })
    }
}

fn get_repeated_name(attrs: &[syn::Attribute]) -> Result<Option<syn::Ident>, syn::Error> {
    for attr in attrs.iter() {
        for segment in attr.path.segments.iter() {
            if segment.ident == "builder" {
                for token in attr.tokens.clone().into_iter() {
                    if let proc_macro2::TokenTree::Group(group) = token {
                        if group.delimiter() == proc_macro2::Delimiter::Parenthesis {
                            let mut stream = group.stream().into_iter();

                            if let (
                                Some(proc_macro2::TokenTree::Ident(ident)),
                                Some(proc_macro2::TokenTree::Punct(equals)),
                                Some(proc_macro2::TokenTree::Literal(repeated_name)),
                            ) = (stream.next(), stream.next(), stream.next())
                            {
                                if ident == "each" && equals.as_char() == '=' {
                                    if let syn::Lit::Str(repeated_name) =
                                        syn::Lit::new(repeated_name)
                                    {
                                        return Ok(Some(format_ident!(
                                            "{}",
                                            repeated_name.value()
                                        )));
                                    }
                                } else {
                                    return Err(syn::Error::new(
                                        group.span(),
                                        "expected `each = '...'`",
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

fn is_option(t: &syn::Type) -> bool {
    match t {
        syn::Type::Path(t) => match t.path.segments.first() {
            Some(t) => t.ident == "Option",
            _ => false,
        },
        _ => false,
    }
}

fn get_angle_bracket_arg(t: &syn::Type) -> Option<&syn::Type> {
    if let syn::Type::Path(t) = t {
        if let Some(t) = t.path.segments.first() {
            if let syn::PathArguments::AngleBracketed(t) = &t.arguments {
                if let Some(t) = t.args.first() {
                    if let syn::GenericArgument::Type(t) = t {
                        return Some(t);
                    }
                }
            }
        }
    }
    None
}

fn derive_setter_functions(fields: &[Field]) -> proc_macro2::TokenStream {
    let setter_functions = fields
        .iter()
        .map(|field| {
            let name = field.name;
            if let Some(repeated_name) = field
                .repeated_name
                .as_ref()
                .expect("Unexpected repeated_name error")
                .as_ref()
            {
                let repeated_type =
                    get_angle_bracket_arg(field.field_type).expect("Expected vector type");
                quote! {
                    fn #repeated_name(&mut self, x: #repeated_type) -> &mut Self{
                        self.#name.push(x);
                        self
                    }
                }
            } else {
                let field_type = field.field_type;
                quote! {
                    fn #name(&mut self, x: #field_type) -> &mut Self{
                        self.#name = std::option::Option::Some(x);
                        self
                    }
                }
            }
        })
        .collect::<Vec<proc_macro2::TokenStream>>();

    quote! {
        #(#setter_functions)*
    }
}

fn derive_build_function(name: &syn::Ident, fields: &[Field]) -> proc_macro2::TokenStream {
    let field_assignments: Vec<proc_macro2::TokenStream> = fields
        .iter()
        .map(|field| {
            let field_name = field.name;
            let field_error_msg = format!("Field '{}' not initialized.", field_name);
            if !field.optional
                && field
                    .repeated_name
                    .as_ref()
                    .expect("Unexpected repeated_name error")
                    .is_none()
            {
                quote! {
                    #field_name: self.#field_name.take().ok_or(#field_error_msg)?
                }
            } else if field
                .repeated_name
                .as_ref()
                .expect("Unexpected repeated_name error")
                .is_some()
            {
                quote! {
                    #field_name: self.#field_name.clone()
                }
            } else {
                quote! {
                    #field_name: self.#field_name.take()
                }
            }
        })
        .collect();

    quote! {
        fn build(&mut self) -> std::result::Result<#name, std::boxed::Box<dyn std::error::Error>> {
            std::result::Result::Ok(#name {
                #(#field_assignments),*
            })
        }
    }
}
