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
