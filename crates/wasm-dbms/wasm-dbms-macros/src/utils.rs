use proc_macro2::Span;
use syn::Ident;

/// Generate an infinite iterator of anonymous identifiers with an optional prefix.
pub fn anon_ident_iter(prefix: Option<&str>) -> impl Iterator<Item = Ident> + Clone + use<'_> {
    let prefix = prefix.unwrap_or("");
    ('a'..='z').cycle().enumerate().map(move |(i, ch)| {
        let wrap = i / 26;
        let name = if wrap == 0 {
            format!("{}{}", prefix, ch)
        } else {
            format!("{}{}{}", prefix, ch, wrap - 1)
        };
        Ident::new(&name, Span::call_site())
    })
}

/// Rejects lifetime and const generic parameters, which the `derive` macro cannot carry.
///
/// Type parameters (with their bounds and the `where` clause) are supported.
///
/// # Errors
///
/// Returns an error spanning the first lifetime or const generic parameter found.
pub fn reject_unsupported_generics(generics: &syn::Generics, derive: &str) -> syn::Result<()> {
    match generics
        .params
        .iter()
        .find(|param| !matches!(param, syn::GenericParam::Type(_)))
    {
        Some(param @ syn::GenericParam::Lifetime(_)) => Err(syn::Error::new_spanned(
            param,
            format!("`{derive}` does not support lifetime parameters"),
        )),
        Some(param) => Err(syn::Error::new_spanned(
            param,
            format!("`{derive}` does not support const generic parameters"),
        )),
        None => Ok(()),
    }
}

/// Returns `generics` with a `T: 'static` predicate added for every type parameter.
///
/// `TableSchema` requires `Self: 'static`, so every impl of the generated table traits needs
/// its type parameters to be `'static`.
pub fn with_static_bounds(generics: &syn::Generics) -> syn::Generics {
    let mut generics = generics.clone();
    let params: Vec<Ident> = generics
        .type_params()
        .map(|param| param.ident.clone())
        .collect();
    if params.is_empty() {
        return generics;
    }
    let where_clause = generics.make_where_clause();
    for param in params {
        where_clause
            .predicates
            .push(syn::parse_quote! { #param: 'static });
    }
    generics
}
