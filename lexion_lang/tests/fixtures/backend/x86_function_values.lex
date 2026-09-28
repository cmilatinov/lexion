fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn apply(callback: fn(i32) -> i32, value: i32) -> i32 {
    return callback(value);
}

fn main() -> i32 {
    let callback = add_one;
    return apply(callback, 4);
}
