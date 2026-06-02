fn read(value: &i32) -> i32 {
    return 1;
}

fn main() -> i32 {
    let value = 1;
    let reference = &value;
    return read(reference);
}
