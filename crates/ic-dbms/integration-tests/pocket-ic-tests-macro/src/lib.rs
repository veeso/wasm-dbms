use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, parse_macro_input};

#[proc_macro_attribute]
pub fn test(_args: TokenStream, input: TokenStream) -> TokenStream {
    let input_fn: ItemFn = parse_macro_input!(input);
    let fn_name = &input_fn.sig.ident;
    let block = &input_fn.block;
    let inputs = &input_fn.sig.inputs;

    let (param_ident, _param_type) = match inputs.iter().next() {
        Some(FnArg::Typed(pat_type)) => match (&*pat_type.pat, &*pat_type.ty) {
            (Pat::Ident(ident), ty) => (ident.ident.clone(), ty.clone()),
            _ => panic!("Unsupported function parameter pattern"),
        },
        _ => panic!("Function must have exactly one argument"),
    };

    let result = quote! {
        #[tokio::test]
        async fn #fn_name() {
            let mut #param_ident = ::pocket_ic_tests::PocketIcTestEnv::init().await;

            {
                #block
            }

            #param_ident.stop().await;
        }
    };

    result.into()
}
