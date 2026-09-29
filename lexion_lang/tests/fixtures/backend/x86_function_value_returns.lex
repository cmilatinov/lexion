fn add_one(value: i32) -> i32 {
    return value + 1;
}

fn make_adder() -> fn(i32) -> i32 {
    return add_one;
}

fn zero() -> i32 {
    return 0;
}

fn apply_zero(handler: fn() -> i32) -> i32 {
    return handler();
}

fn main() -> i32 {
    let handler = make_adder();
    return make_adder()(4) + handler(1) + apply_zero(zero);
}
