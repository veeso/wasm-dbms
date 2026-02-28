use candid::Principal;

pub fn alice() -> Principal {
    Principal::from_text("be2us-64aaa-aaaaa-qaabq-cai").unwrap()
}

pub fn admin() -> Principal {
    Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap()
}

pub fn bob() -> Principal {
    Principal::from_text("bs5l3-6b3zu-dpqyj-p2x4a-jyg4k-goneb-afof2-y5d62-skt67-3756q-dqe").unwrap()
}
