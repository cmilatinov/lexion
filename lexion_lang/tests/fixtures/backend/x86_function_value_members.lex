struct Holder {
    handler: fn(i32) -> i32
}

fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    let holder = Holder { handler: add_one };
    holder.handler = add_one;
    return holder.handler(4);
}
