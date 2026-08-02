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
    let first = 1;
    let second = 2;
    let third = 3;
    let fourth = 4;
    let fifth = 5;
    let sixth = 6;
    let seventh = 7;
    let returned = select_last(
        &first,
        &second,
        &third,
        &fourth,
        &fifth,
        &sixth,
        &seventh
    );
    *returned = 9;
    return seventh;
}
