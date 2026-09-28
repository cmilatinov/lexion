fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let value = 4;
    let next = add_one(value);
    return next + value;
}
