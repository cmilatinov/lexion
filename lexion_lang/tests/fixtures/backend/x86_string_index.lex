fn literal_index() -> char {
    return "xyz"[2];
}

fn parameter_index(text: &str, index: i32) -> char {
    return text[index];
}

fn main() -> char {
    let text = "abc";
    let index = 1;
    return parameter_index(text, index);
}
