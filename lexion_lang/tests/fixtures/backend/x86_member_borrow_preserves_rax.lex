struct Values {
    first: i32
}

fn main() -> i32 {
    let values = Values { first: 3 };
    return (values.first + 7) + *(&values.first);
}
