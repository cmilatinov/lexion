fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn apply(handler: fn(i32) -> i32, value: i32) -> i32 {
    return handler(value);
}

fn main() -> i32 {
    let handler = add_one;
    return apply(handler, 4);
}
