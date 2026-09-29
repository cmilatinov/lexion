fn borrow_parameter(text: &str, index: i32) -> char {
    let second = &text[index];
    return *second;
}

fn main() -> char {
    let text = "abc";
    return borrow_parameter(text, 1);
}
