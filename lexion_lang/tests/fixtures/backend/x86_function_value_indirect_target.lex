fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let handler = add_one;
    return handler(4);
}
