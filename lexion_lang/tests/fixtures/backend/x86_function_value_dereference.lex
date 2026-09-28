fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let callback = add_one;
    let callback_ref = &callback;
    *callback_ref = add_one;
    return (*callback_ref)(4);
}
