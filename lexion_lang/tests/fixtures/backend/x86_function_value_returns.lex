fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn make_adder() -> fn(i32) -> i32 {
    return add_one;
}

fn zero() -> i32 {
    return 0;
}

fn apply_zero(callback: fn() -> i32) -> i32 {
    return callback();
}

fn main() -> i32 {
    let callback = make_adder();
    return make_adder()(4) + callback(1) + apply_zero(zero);
}
