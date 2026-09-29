fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let handler = add_one;
    let handler_ref = &handler;
    let left = 40;
    let right = 2;
    *handler_ref = add_one;
    return right + left;
}
