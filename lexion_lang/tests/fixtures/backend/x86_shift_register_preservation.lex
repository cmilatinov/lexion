fn main(a: i32, b: i32, c: i32, d: i32) -> i32 {
    let count = a + 1;
    let left = b + 2;
    let keep1 = c + 3;
    let keep2 = d + 4;
    let shifted = left << count;
    return shifted + keep1 + keep2 + count;
}
