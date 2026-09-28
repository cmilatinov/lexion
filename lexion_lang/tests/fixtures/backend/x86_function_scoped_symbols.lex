fn scalar_value() -> i32 {
    let value = 1;
    return value;
}

fn reference_value() -> i32 {
    let source = 1;
    let value = &source;
    *value = 2;
    return *value;
}
