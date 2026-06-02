fn main() -> i32 {
    let mask = (14 & 11) | (1 << 3);
    let shifted = mask >> 1;
    return shifted ^ ~2;
}
