struct Holder {
    callback: fn(i32) -> i32
}

fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let value = 4;
    let callback = add_one;
    let holder = Holder { callback: callback };
    return value;
}
