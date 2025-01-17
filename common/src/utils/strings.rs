use solana_sdk::pubkey::Pubkey;

pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for c in s.chars() {
        if c == '_' {
            capitalize = true;
        } else if capitalize {
            result.push(c.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(c);
        }
    }
    result
}

pub fn shorten_address(key: &Pubkey) -> String {
    // shorten address to 4 characters ... 4 characters
    let str = key.to_string();
    let first_4_chars = &str[0..4];
    let middle_3_dots = "...";
    let last_4_chars = &str[str.len() - 4..];
    format!("{}{}{}", first_4_chars, middle_3_dots, last_4_chars)
}
