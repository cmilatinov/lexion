fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn pick() -> fn(i32) -> i32 {
    return add_one;
}

fn main() -> i32 {
    let callback = add_one;
    let callback_ref = &callback;
    *callback_ref = pick();
    return callback(4);
}
