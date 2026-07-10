struct Point {
    x: i32
}

extern fn make_point() -> Point;

fn member_address() -> i32 {
    let point = make_point();
    point.x = 7;
    return point.x;
}

fn index_address() -> char {
    let text = "abc";
    text[0] = text[1];
    return text[0];
}

fn reference_address() -> i32 {
    let value = 1;
    let reference = &value;
    *reference = 2;
    return *reference;
}
