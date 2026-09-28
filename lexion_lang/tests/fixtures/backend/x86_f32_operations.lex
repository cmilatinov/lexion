fn main() -> i32 {
    let left: f32 = 3.0;
    let right: f32 = 2.0;
    let sum = left + right;
    let difference = sum - right;
    let product = difference * right;
    let quotient = product / right;
    let negated = -quotient;
    let less = negated < 0.0;
    let greater = quotient >= 3.0;
    let result = if less && greater { 0 } else { 1 };
    return result;
}
