fn identity(input: &i32) -> &i32 {
    return input;
}

fn main() -> i32 {
    let value = 1;
    let reference = &value;
    let returned = identity(reference);
    *returned = 5;
    return value;
}
