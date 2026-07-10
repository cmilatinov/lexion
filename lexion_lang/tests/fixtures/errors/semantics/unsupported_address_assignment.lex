struct Point {
    x: i32
}
extern fn point() -> Point;
fn member_assignment() {
    let value = point();
    value.x = 1;
}
fn index_assignment() {
    let text = "abc";
    text[0] = text[1];
}
fn dereference_assignment() {
    let value = 1;
    let reference = &value;
    *reference = 2;
}
fn address_of_assignment() {
    let value = 1;
    &value = 2;
}
