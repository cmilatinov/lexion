fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let callback = add_one;
    let callback_ref = &callback;
    let left = 40;
    let right = 2;
    *callback_ref = add_one;
    return right + left;
}
