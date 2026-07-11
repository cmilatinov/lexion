fn select_last(
    a: &i32,
    b: &i32,
    c: &i32,
    d: &i32,
    e: &i32,
    f: &i32,
    g: &i32
) -> &i32 {
    return g;
}

fn main() -> i32 {
    let value = 1;
    let reference = &value;
    let returned = select_last(
        reference,
        reference,
        reference,
        reference,
        reference,
        reference,
        reference
    );
    *returned = 6;
    return value;
}
