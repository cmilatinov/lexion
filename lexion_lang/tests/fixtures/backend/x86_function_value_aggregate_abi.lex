struct Holder {
    callback: fn(i32) -> i32
}

fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn relay(holder: Holder) -> Holder {
    return holder;
}

fn main() -> i32 {
    let first = 1;
    let second = 2;
    let third = 3;
    let fourth = 4;
    let fifth = 5;
    let sixth = 6;
    let seventh = 7;
    let eighth = 8;
    let ninth = 9;
    let tenth = 10;
    let callback = add_one;
    let holder = relay(Holder { callback: callback });
    return first + second + third + fourth + fifth + sixth + seventh + eighth + ninth + tenth
        + holder.callback(0);
}
