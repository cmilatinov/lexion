struct Holder {
    handler: fn(i32) -> i32
}

fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn get_callback(holder: Holder) -> fn(i32) -> i32 {
    return holder.handler;
}

fn main() -> i32 {
    return get_callback(Holder { handler: add_one })(4);
}
