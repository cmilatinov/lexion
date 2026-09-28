fn update_char(value: char, replacement: char) -> char {
    let reference = &value;
    *reference = replacement;
    return *reference;
}

fn main() -> i32 {
    let value = true;
    let reference = &value;
    *reference = false;
    let result = if *reference { 1 } else { 0 };
    return result;
}
