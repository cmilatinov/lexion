struct Inner {
    pointer: &i32
}

struct Outer {
    inner: Inner
}

fn identity(value: Outer) -> Outer {
    return value;
}

fn main() -> i32 {
    let number = 7;
    let outer = Outer { inner: Inner { pointer: &number } };
    let returned = identity(outer);
    return number;
}
