fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let callback = add_one;
    let value = 4;
    return callback(value);
}
