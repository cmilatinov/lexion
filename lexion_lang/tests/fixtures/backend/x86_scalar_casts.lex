fn main() -> i32 {
    let as_int = true as i32;
    let as_bool = 2 as bool;
    let bool_int = as_bool as i32;
    let same = as_int as i32;
    return same + bool_int;
}
