fn apply(callback: fn(i32) -> i32, value: i32) -> i32 {
    return callback(value);
}

fn main() -> i32 {
    fn add_one(value: i32) -> i32 {
        return value + 1;
    }

    return apply(add_one, 4);
}
