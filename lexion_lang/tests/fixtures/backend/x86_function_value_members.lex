struct Holder {
    callback: fn(i32) -> i32
}

fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let holder = Holder { callback: add_one };
    holder.callback = add_one;
    return holder.callback(4);
}
