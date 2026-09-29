fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let handler = add_one;
    let handler_ref = &handler;
    *handler_ref = add_one;
    return (*handler_ref)(4);
}
