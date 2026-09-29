fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn pick() -> fn(i32) -> i32 {
    return add_one;
}

fn main() -> i32 {
    let handler = add_one;
    let handler_ref = &handler;
    *handler_ref = pick();
    return handler(4);
}
