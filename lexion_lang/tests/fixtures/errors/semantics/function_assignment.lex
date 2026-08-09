fn f(value: i32) -> i32 {
    return value;
}

fn g(value: i32) -> i32 {
    return value + 1;
}

fn main() -> i32 {
    f = g;
    return f(1);
}
