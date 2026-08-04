struct Point {
    x: i32,
    y: i32
}

fn main() {
    let unknown = Missing { x: 1 };
    let invalid = Point {
        x: false,
        x: 2,
        z: 3,
    };
}
