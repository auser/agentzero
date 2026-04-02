//! Implementation of `#[tool_fn]` — generates a full `Tool` impl from an async function.
//!
//! Transforms:
//! ```ignore
//! /// Reverse the input string.
//! #[tool_fn(name = "reverse_string")]
//! async fn reverse_string(
//!     /// The text to reverse
//!     text: String,
//!     #[ctx] ctx: &ToolContext,
//! ) -> anyhow::Result<ToolResult> {
//!     Ok(ToolResult::text(text.chars().rev().collect::<String>()))
//! }
//! ```
//!
//! Into: input struct, tool struct, `Tool` trait impl, and inner function.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, FnArg, Ident, ItemFn, LitStr, Pat, Token, Type};

use crate::tool_schema::{
    extract_doc_comment, extract_schema_enum_values, extract_schema_items_type,
    has_serde_default_attr, rust_type_to_json_type,
};

// ---------------------------------------------------------------------------
// Attribute parsing: #[tool_fn(name = "...", description = "...")]
// ---------------------------------------------------------------------------

struct ToolFnArgs {
    name: LitStr,
    description: Option<LitStr>,
}

impl Parse for ToolFnArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name: Option<LitStr> = None;
        let mut description: Option<LitStr> = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: LitStr = input.parse()?;

            match key.to_string().as_str() {
                "name" => name = Some(value),
                "description" => description = Some(value),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown tool_fn attribute: `{other}` (expected `name` or `description`)"),
                    ));
                }
            }

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        let name = name.ok_or_else(|| input.error("missing `name` attribute"))?;
        Ok(ToolFnArgs { name, description })
    }
}

// ---------------------------------------------------------------------------
// Classified function parameters
// ---------------------------------------------------------------------------

struct ClassifiedParam {
    ident: Ident,
    ty: Type,
    doc: Option<String>,
    /// Forwarded attributes (e.g. `#[serde(default)]`, `#[schema(...)]`)
    forwarded_attrs: Vec<Attribute>,
}

struct ClassifiedParams {
    /// Regular input params → become fields on the input struct
    inputs: Vec<ClassifiedParam>,
    /// The `#[ctx]` parameter (ident + type)
    ctx: Option<(Ident, Type)>,
    /// The `#[state]` parameter (ident + type)
    state: Option<(Ident, Type)>,
}

fn has_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|a| a.path().is_ident(name))
}

fn classify_params(func: &ItemFn) -> syn::Result<ClassifiedParams> {
    let mut inputs = Vec::new();
    let mut ctx = None;
    let mut state = None;

    for arg in &func.sig.inputs {
        let FnArg::Typed(pat_type) = arg else {
            return Err(syn::Error::new_spanned(
                arg,
                "#[tool_fn] does not support `self` parameters",
            ));
        };

        let Pat::Ident(pat_ident) = pat_type.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &pat_type.pat,
                "#[tool_fn] requires named parameters",
            ));
        };

        let ident = pat_ident.ident.clone();
        let ty = (*pat_type.ty).clone();
        let attrs = &pat_type.attrs;

        if has_attr(attrs, "ctx") {
            if ctx.is_some() {
                return Err(syn::Error::new_spanned(
                    pat_type,
                    "duplicate #[ctx] parameter",
                ));
            }
            ctx = Some((ident, ty));
        } else if has_attr(attrs, "state") {
            if state.is_some() {
                return Err(syn::Error::new_spanned(
                    pat_type,
                    "duplicate #[state] parameter",
                ));
            }
            state = Some((ident, ty));
        } else {
            // Forward serde/schema attrs; strip doc attrs for extraction.
            let forwarded: Vec<Attribute> = attrs
                .iter()
                .filter(|a| a.path().is_ident("serde") || a.path().is_ident("schema"))
                .cloned()
                .collect();
            let doc = extract_doc_comment(attrs);
            inputs.push(ClassifiedParam {
                ident,
                ty,
                doc,
                forwarded_attrs: forwarded,
            });
        }
    }

    Ok(ClassifiedParams { inputs, ctx, state })
}

// ---------------------------------------------------------------------------
// Code generation
// ---------------------------------------------------------------------------

/// Convert a snake_case function name to PascalCase + "Tool" suffix.
fn to_tool_struct_name(fn_name: &Ident) -> Ident {
    let snake = fn_name.to_string();
    let pascal: String = snake
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    format_ident!("{}Tool", pascal)
}

fn to_input_struct_name(fn_name: &Ident) -> Ident {
    let snake = fn_name.to_string();
    let pascal: String = snake
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    format_ident!("{}Input", pascal)
}

pub fn expand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as ToolFnArgs);
    let func = syn::parse_macro_input!(item as ItemFn);

    match expand_inner(args, func) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_inner(args: ToolFnArgs, func: ItemFn) -> syn::Result<TokenStream2> {
    let classified = classify_params(&func)?;
    let fn_name = &func.sig.ident;
    let tool_name_lit = &args.name;

    // Description: explicit attribute > doc comment on function > empty
    let fn_doc = extract_doc_comment(&func.attrs);
    let description_str = if let Some(ref desc) = args.description {
        desc.value()
    } else {
        fn_doc.unwrap_or_default()
    };

    let tool_struct = to_tool_struct_name(fn_name);
    let input_struct = to_input_struct_name(fn_name);
    let inner_fn = format_ident!("__{}_impl", fn_name);

    let has_inputs = !classified.inputs.is_empty();
    let has_state = classified.state.is_some();

    // --- Generate input struct ---
    let input_struct_def = if has_inputs {
        let fields: Vec<TokenStream2> = classified
            .inputs
            .iter()
            .map(|p| {
                let ident = &p.ident;
                let ty = &p.ty;
                let attrs = &p.forwarded_attrs;
                let doc_attr = p.doc.as_ref().map(|d| quote! { #[doc = #d] });
                quote! {
                    #doc_attr
                    #(#attrs)*
                    pub #ident: #ty,
                }
            })
            .collect();

        Some(quote! {
            #[derive(serde::Deserialize)]
            #[allow(dead_code)]
            struct #input_struct {
                #(#fields)*
            }
        })
    } else {
        None
    };

    // --- Generate JSON schema ---
    let schema_impl = if has_inputs {
        let mut property_entries = Vec::new();
        let mut required_entries = Vec::new();

        for p in &classified.inputs {
            let field_name_str = p.ident.to_string();
            let (json_type, is_optional, is_array) = rust_type_to_json_type(&p.ty);

            let enum_values = extract_schema_enum_values(&p.forwarded_attrs);
            let items_type_override = extract_schema_items_type(&p.forwarded_attrs);
            let has_serde_default = has_serde_default_attr(&p.forwarded_attrs);

            let type_token = quote! { "type": #json_type };

            let desc_token = if let Some(ref desc) = p.doc {
                quote! { , "description": #desc }
            } else {
                quote! {}
            };

            let enum_token = if let Some(ref vals) = enum_values {
                let val_tokens: Vec<_> = vals.iter().map(|v| quote! { #v }).collect();
                quote! { , "enum": [#(#val_tokens),*] }
            } else {
                quote! {}
            };

            let items_token = if is_array {
                let items_type = items_type_override.as_deref().unwrap_or("string");
                quote! { , "items": { "type": #items_type } }
            } else {
                quote! {}
            };

            property_entries.push(quote! {
                #field_name_str: {
                    #type_token
                    #desc_token
                    #enum_token
                    #items_token
                }
            });

            if !is_optional && !has_serde_default {
                required_entries.push(quote! { #field_name_str });
            }
        }

        quote! {
            fn input_schema(&self) -> Option<serde_json::Value> {
                Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        #(#property_entries),*
                    },
                    "required": [#(#required_entries),*]
                }))
            }
        }
    } else {
        quote! {
            fn input_schema(&self) -> Option<serde_json::Value> {
                None
            }
        }
    };

    // --- Generate tool struct ---
    let (struct_def, constructor) = if has_state {
        let (state_ident, state_ty) = classified.state.as_ref().expect("checked above");
        // Strip leading & or &mut from type for the struct field
        let owned_ty = strip_reference(state_ty);
        let _ = state_ident; // used in inner fn call
        (
            quote! {
                pub struct #tool_struct {
                    state: #owned_ty,
                }
            },
            quote! {
                impl #tool_struct {
                    pub fn new(state: #owned_ty) -> Self {
                        Self { state }
                    }
                }
            },
        )
    } else {
        (
            quote! {
                #[derive(Debug, Default, Clone, Copy)]
                pub struct #tool_struct;
            },
            quote! {},
        )
    };

    // --- Generate execute body ---
    let ctx_ident = classified.ctx.as_ref().map(|(id, _)| id.clone());

    let execute_body = if has_inputs {
        let field_idents: Vec<&Ident> = classified.inputs.iter().map(|p| &p.ident).collect();

        let inner_args = build_inner_call_args(&classified, &ctx_ident);

        quote! {
            let __parsed: #input_struct = serde_json::from_str(input)
                .map_err(|e| anyhow::anyhow!("invalid input for tool '{}': {e}", #tool_name_lit))?;
            let #input_struct { #(#field_idents),* , .. } = __parsed;
            #inner_fn(#(#inner_args),*).await
        }
    } else {
        let inner_args = build_inner_call_args(&classified, &ctx_ident);
        quote! {
            #inner_fn(#(#inner_args),*).await
        }
    };

    // --- Generate inner function ---
    let inner_fn_params = build_inner_fn_params(&func, &classified);
    let ret_ty = &func.sig.output;
    let body = &func.block;
    let asyncness = &func.sig.asyncness;

    // --- Assemble ---
    let expanded = quote! {
        #input_struct_def

        #struct_def

        #constructor

        #[allow(unused_variables)]
        #asyncness fn #inner_fn(#(#inner_fn_params),*) #ret_ty
            #body

        #[async_trait::async_trait]
        impl agentzero_core::Tool for #tool_struct {
            fn name(&self) -> &'static str {
                #tool_name_lit
            }

            fn description(&self) -> &'static str {
                #description_str
            }

            #schema_impl

            async fn execute(&self, input: &str, ctx: &agentzero_core::ToolContext) -> anyhow::Result<agentzero_core::ToolResult> {
                #execute_body
            }
        }
    };

    Ok(expanded)
}

/// Build the parameter list for the inner function (preserving original types).
fn build_inner_fn_params(_func: &ItemFn, classified: &ClassifiedParams) -> Vec<TokenStream2> {
    let mut params = Vec::new();

    // Input params (owned values)
    for p in &classified.inputs {
        let ident = &p.ident;
        let ty = &p.ty;
        params.push(quote! { #ident: #ty });
    }

    // State param
    if let Some((ident, ty)) = &classified.state {
        params.push(quote! { #ident: #ty });
    }

    // Ctx param — always &ToolContext regardless of what the user wrote
    if let Some((ident, _)) = &classified.ctx {
        params.push(quote! { #ident: &agentzero_core::ToolContext });
    }

    params
}

/// Build the argument list for calling the inner function from execute().
fn build_inner_call_args(
    classified: &ClassifiedParams,
    ctx_ident: &Option<Ident>,
) -> Vec<TokenStream2> {
    let mut args: Vec<TokenStream2> = Vec::new();

    // Input params (moved from destructured struct)
    for p in &classified.inputs {
        let ident = &p.ident;
        args.push(quote! { #ident });
    }

    // State param
    if classified.state.is_some() {
        args.push(quote! { &self.state });
    }

    // Ctx param
    if ctx_ident.is_some() {
        args.push(quote! { ctx });
    }

    args
}

/// Strip `&` or `&mut` from a type to get the owned version for struct fields.
fn strip_reference(ty: &Type) -> TokenStream2 {
    match ty {
        Type::Reference(r) => {
            let inner = &r.elem;
            quote! { #inner }
        }
        _ => quote! { #ty },
    }
}
