struct Holder {
    handler: fn(i32) -> i32
}

fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let value = 4;
    let handler = add_one;
    let holder = Holder { handler: handler };
    return value;
}
