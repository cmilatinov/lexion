fn literal_index() -> char {
    return "xyz"[2];
}

fn parameter_index(text: &str, index: i32) -> char {
    return text[index];
}

fn make_index() -> i32 {
    return 1;
}

fn allocated_index() -> char {
    let text = "abc";
    let index = make_index();
    return text[index];
}

fn main() -> char {
    let text = "abc";
    let index = 1;
    let from_parameter = parameter_index(text, index);
    return allocated_index();
}
