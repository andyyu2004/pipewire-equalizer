// use serde::{Deserialize, Deserializer, Serialize, de, ser};
// use spa_json::{json, to_string, to_string_pretty};
// use std::collections::BTreeMap;
// use std::fmt::Debug;

// macro_rules! treemap {
//     () => {
//         BTreeMap::new()
//     };
//     ($($k:expr => $v:expr),+ $(,)?) => {
//         {
//             let mut m = BTreeMap::new();
//             $(
//                 m.insert($k, $v);
//             )+
//             m
//         }
//     };
// }

// #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
// #[serde(deny_unknown_fields)]
// enum Animal {
//     Dog,
//     Frog(String, Vec<isize>),
//     Cat { age: usize, name: String },
//     AntHive(Vec<String>),
// }
//
// #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
// struct Inner {
//     a: (),
//     b: usize,
//     c: Vec<String>,
// }
//
// #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
// struct Outer {
//     inner: Vec<Inner>,
// }
//
// fn test_encode_ok<T>(errors: &[(T, &str)])
// where
//     T: PartialEq + Debug + ser::Serialize,
// {
//     for &(ref value, out) in errors {
//         let out = out.to_string();
//
//         let s = to_string(value).unwrap();
//         assert_eq!(s, out);
//
//         let v = to_value(value).unwrap();
//         let s = to_string(&v).unwrap();
//         assert_eq!(s, out);
//     }
// }
//
// fn test_pretty_encode_ok<T>(errors: &[(T, &str)])
// where
//     T: PartialEq + Debug + ser::Serialize,
// {
//     for &(ref value, out) in errors {
//         let out = out.to_string();
//
//         let s = to_string_pretty(value).unwrap();
//         assert_eq!(s, out);
//
//         let v = to_value(value).unwrap();
//         let s = to_string_pretty(&v).unwrap();
//         assert_eq!(s, out);
//     }
// }
//
// #[test]
// fn test_write_null() {
//     let tests = &[((), "null")];
//     test_encode_ok(tests);
//     test_pretty_encode_ok(tests);
// }
//
// #[test]
// fn test_write_u64() {
//     let tests = &[(3u64, "3"), (u64::MAX, &u64::MAX.to_string())];
//     test_encode_ok(tests);
//     test_pretty_encode_ok(tests);
// }
//
// #[test]
// fn test_write_i64() {
//     let tests = &[
//         (3i64, "3"),
//         (-2i64, "-2"),
//         (-1234i64, "-1234"),
//         (i64::MIN, &i64::MIN.to_string()),
//     ];
//     test_encode_ok(tests);
//     test_pretty_encode_ok(tests);
// }
//
// #[test]
// fn test_write_f64() {
//     let tests = &[
//         (3.0, "3.0"),
//         (3.1, "3.1"),
//         (-1.5, "-1.5"),
//         (0.5, "0.5"),
//         (f64::MIN, "-1.7976931348623157e308"),
//         (f64::MAX, "1.7976931348623157e308"),
//         (f64::EPSILON, "2.220446049250313e-16"),
//     ];
//     test_encode_ok(tests);
//     test_pretty_encode_ok(tests);
// }
//
// #[test]
// fn test_encode_nonfinite_float_yields_null() {
//     let v = to_value(f64::NAN.copysign(1.0)).unwrap();
//     assert!(v.is_null());
//
//     let v = to_value(f64::NAN.copysign(-1.0)).unwrap();
//     assert!(v.is_null());
//
//     let v = to_value(f64::INFINITY).unwrap();
//     assert!(v.is_null());
//
//     let v = to_value(-f64::INFINITY).unwrap();
//     assert!(v.is_null());
//
//     let v = to_value(f32::NAN.copysign(1.0)).unwrap();
//     assert!(v.is_null());
//
//     let v = to_value(f32::NAN.copysign(-1.0)).unwrap();
//     assert!(v.is_null());
//
//     let v = to_value(f32::INFINITY).unwrap();
//     assert!(v.is_null());
//
//     let v = to_value(-f32::INFINITY).unwrap();
//     assert!(v.is_null());
// }
//
// #[test]
// fn test_write_str() {
//     let tests = &[("", "\"\""), ("foo", "\"foo\"")];
//     test_encode_ok(tests);
//     test_pretty_encode_ok(tests);
// }
//
// #[test]
// fn test_write_bool() {
//     let tests = &[(true, "true"), (false, "false")];
//     test_encode_ok(tests);
//     test_pretty_encode_ok(tests);
// }
//
// #[test]
// fn test_write_char() {
//     let tests = &[
//         ('n', "\"n\""),
//         ('"', "\"\\\"\""),
//         ('\\', "\"\\\\\""),
//         ('/', "\"/\""),
//         ('\x08', "\"\\b\""),
//         ('\x0C', "\"\\f\""),
//         ('\n', "\"\\n\""),
//         ('\r', "\"\\r\""),
//         ('\t', "\"\\t\""),
//         ('\x0B', "\"\\u000b\""),
//         ('\u{3A3}', "\"\u{3A3}\""),
//     ];
//     test_encode_ok(tests);
//     test_pretty_encode_ok(tests);
// }
//
// #[test]
// fn test_write_list() {
//     test_encode_ok(&[
//         (vec![], "[]"),
//         (vec![true], "[true]"),
//         (vec![true, false], "[true,false]"),
//     ]);
//
//     test_encode_ok(&[
//         (vec![vec![], vec![], vec![]], "[[],[],[]]"),
//         (vec![vec![1, 2, 3], vec![], vec![]], "[[1,2,3],[],[]]"),
//         (vec![vec![], vec![1, 2, 3], vec![]], "[[],[1,2,3],[]]"),
//         (vec![vec![], vec![], vec![1, 2, 3]], "[[],[],[1,2,3]]"),
//     ]);
//
//     test_pretty_encode_ok(&[
//         (vec![vec![], vec![], vec![]], pretty_str!([[], [], []])),
//         (
//             vec![vec![1, 2, 3], vec![], vec![]],
//             pretty_str!([[1, 2, 3], [], []]),
//         ),
//         (
//             vec![vec![], vec![1, 2, 3], vec![]],
//             pretty_str!([[], [1, 2, 3], []]),
//         ),
//         (
//             vec![vec![], vec![], vec![1, 2, 3]],
//             pretty_str!([[], [], [1, 2, 3]]),
//         ),
//     ]);
//
//     test_pretty_encode_ok(&[
//         (vec![], "[]"),
//         (vec![true], pretty_str!([true])),
//         (vec![true, false], pretty_str!([true, false])),
//     ]);
//
//     let long_test_list = json!([false, null, ["foo\nbar", 3.5]]);
//
//     test_encode_ok(&[(
//         long_test_list.clone(),
//         json_str!([false, null, ["foo\nbar", 3.5]]),
//     )]);
//
//     test_pretty_encode_ok(&[(
//         long_test_list,
//         pretty_str!([false, null, ["foo\nbar", 3.5]]),
//     )]);
// }
//
// #[test]
// fn test_write_object() {
//     test_encode_ok(&[
//         (treemap!(), "{}"),
//         (treemap!("a".to_owned() => true), "{\"a\":true}"),
//         (
//             treemap!(
//                 "a".to_owned() => true,
//                 "b".to_owned() => false,
//             ),
//             "{\"a\":true,\"b\":false}",
//         ),
//     ]);
//
//     test_encode_ok(&[
//         (
//             treemap![
//                 "a".to_owned() => treemap![],
//                 "b".to_owned() => treemap![],
//                 "c".to_owned() => treemap![],
//             ],
//             "{\"a\":{},\"b\":{},\"c\":{}}",
//         ),
//         (
//             treemap![
//                 "a".to_owned() => treemap![
//                     "a".to_owned() => treemap!["a" => vec![1,2,3]],
//                     "b".to_owned() => treemap![],
//                     "c".to_owned() => treemap![],
//                 ],
//                 "b".to_owned() => treemap![],
//                 "c".to_owned() => treemap![],
//             ],
//             "{\"a\":{\"a\":{\"a\":[1,2,3]},\"b\":{},\"c\":{}},\"b\":{},\"c\":{}}",
//         ),
//         (
//             treemap![
//                 "a".to_owned() => treemap![],
//                 "b".to_owned() => treemap![
//                     "a".to_owned() => treemap!["a" => vec![1,2,3]],
//                     "b".to_owned() => treemap![],
//                     "c".to_owned() => treemap![],
//                 ],
//                 "c".to_owned() => treemap![],
//             ],
//             "{\"a\":{},\"b\":{\"a\":{\"a\":[1,2,3]},\"b\":{},\"c\":{}},\"c\":{}}",
//         ),
//         (
//             treemap![
//                 "a".to_owned() => treemap![],
//                 "b".to_owned() => treemap![],
//                 "c".to_owned() => treemap![
//                     "a".to_owned() => treemap!["a" => vec![1,2,3]],
//                     "b".to_owned() => treemap![],
//                     "c".to_owned() => treemap![],
//                 ],
//             ],
//             "{\"a\":{},\"b\":{},\"c\":{\"a\":{\"a\":[1,2,3]},\"b\":{},\"c\":{}}}",
//         ),
//     ]);
//
//     test_encode_ok(&[(treemap!['c' => ()], "{\"c\":null}")]);
//
//     test_pretty_encode_ok(&[
//         (
//             treemap![
//                 "a".to_owned() => treemap![],
//                 "b".to_owned() => treemap![],
//                 "c".to_owned() => treemap![],
//             ],
//             pretty_str!({
//                 "a": {},
//                 "b": {},
//                 "c": {}
//             }),
//         ),
//         (
//             treemap![
//                 "a".to_owned() => treemap![
//                     "a".to_owned() => treemap!["a" => vec![1,2,3]],
//                     "b".to_owned() => treemap![],
//                     "c".to_owned() => treemap![],
//                 ],
//                 "b".to_owned() => treemap![],
//                 "c".to_owned() => treemap![],
//             ],
//             pretty_str!({
//                 "a": {
//                     "a": {
//                         "a": [
//                             1,
//                             2,
//                             3
//                         ]
//                     },
//                     "b": {},
//                     "c": {}
//                 },
//                 "b": {},
//                 "c": {}
//             }),
//         ),
//         (
//             treemap![
//                 "a".to_owned() => treemap![],
//                 "b".to_owned() => treemap![
//                     "a".to_owned() => treemap!["a" => vec![1,2,3]],
//                     "b".to_owned() => treemap![],
//                     "c".to_owned() => treemap![],
//                 ],
//                 "c".to_owned() => treemap![],
//             ],
//             pretty_str!({
//                 "a": {},
//                 "b": {
//                     "a": {
//                         "a": [
//                             1,
//                             2,
//                             3
//                         ]
//                     },
//                     "b": {},
//                     "c": {}
//                 },
//                 "c": {}
//             }),
//         ),
//         (
//             treemap![
//                 "a".to_owned() => treemap![],
//                 "b".to_owned() => treemap![],
//                 "c".to_owned() => treemap![
//                     "a".to_owned() => treemap!["a" => vec![1,2,3]],
//                     "b".to_owned() => treemap![],
//                     "c".to_owned() => treemap![],
//                 ],
//             ],
//             pretty_str!({
//                 "a": {},
//                 "b": {},
//                 "c": {
//                     "a": {
//                         "a": [
//                             1,
//                             2,
//                             3
//                         ]
//                     },
//                     "b": {},
//                     "c": {}
//                 }
//             }),
//         ),
//     ]);
//
//     test_pretty_encode_ok(&[
//         (treemap!(), "{}"),
//         (
//             treemap!("a".to_owned() => true),
//             pretty_str!({
//                 "a": true
//             }),
//         ),
//         (
//             treemap!(
//                 "a".to_owned() => true,
//                 "b".to_owned() => false,
//             ),
//             pretty_str!( {
//                 "a": true,
//                 "b": false
//             }),
//         ),
//     ]);
//
//     let complex_obj = json!({
//         "b": [
//             {"c": "\x0c\x1f\r"},
//             {"d": ""}
//         ]
//     });
//
//     test_encode_ok(&[(
//         complex_obj.clone(),
//         json_str!({
//             "b": [
//                 {
//                     "c": (r#""\f\u001f\r""#)
//                 },
//                 {
//                     "d": ""
//                 }
//             ]
//         }),
//     )]);
//
//     test_pretty_encode_ok(&[(
//         complex_obj,
//         pretty_str!({
//             "b": [
//                 {
//                     "c": (r#""\f\u001f\r""#)
//                 },
//                 {
//                     "d": ""
//                 }
//             ]
//         }),
//     )]);
// }
//
// #[test]
// fn test_write_tuple() {
//     test_encode_ok(&[((5,), "[5]")]);
//
//     test_pretty_encode_ok(&[((5,), pretty_str!([5]))]);
//
//     test_encode_ok(&[((5, (6, "abc")), "[5,[6,\"abc\"]]")]);
//
//     test_pretty_encode_ok(&[((5, (6, "abc")), pretty_str!([5, [6, "abc"]]))]);
// }
//
// #[test]
// fn test_write_enum() {
//     test_encode_ok(&[
//         (Animal::Dog, "\"Dog\""),
//         (
//             Animal::Frog("Henry".to_owned(), vec![]),
//             "{\"Frog\":[\"Henry\",[]]}",
//         ),
//         (
//             Animal::Frog("Henry".to_owned(), vec![349]),
//             "{\"Frog\":[\"Henry\",[349]]}",
//         ),
//         (
//             Animal::Frog("Henry".to_owned(), vec![349, 102]),
//             "{\"Frog\":[\"Henry\",[349,102]]}",
//         ),
//         (
//             Animal::Cat {
//                 age: 5,
//                 name: "Kate".to_owned(),
//             },
//             "{\"Cat\":{\"age\":5,\"name\":\"Kate\"}}",
//         ),
//         (
//             Animal::AntHive(vec!["Bob".to_owned(), "Stuart".to_owned()]),
//             "{\"AntHive\":[\"Bob\",\"Stuart\"]}",
//         ),
//     ]);
//
//     test_pretty_encode_ok(&[
//         (Animal::Dog, "\"Dog\""),
//         (
//             Animal::Frog("Henry".to_owned(), vec![]),
//             pretty_str!({
//                 "Frog": [
//                     "Henry",
//                     []
//                 ]
//             }),
//         ),
//         (
//             Animal::Frog("Henry".to_owned(), vec![349]),
//             pretty_str!({
//                 "Frog": [
//                     "Henry",
//                     [
//                         349
//                     ]
//                 ]
//             }),
//         ),
//         (
//             Animal::Frog("Henry".to_owned(), vec![349, 102]),
//             pretty_str!({
//                 "Frog": [
//                     "Henry",
//                     [
//                       349,
//                       102
//                     ]
//                 ]
//             }),
//         ),
//     ]);
// }
//
// #[test]
// fn test_write_option() {
//     test_encode_ok(&[(None, "null"), (Some("jodhpurs"), "\"jodhpurs\"")]);
//
//     test_encode_ok(&[
//         (None, "null"),
//         (Some(vec!["foo", "bar"]), "[\"foo\",\"bar\"]"),
//     ]);
//
//     test_pretty_encode_ok(&[(None, "null"), (Some("jodhpurs"), "\"jodhpurs\"")]);
//
//     test_pretty_encode_ok(&[
//         (None, "null"),
//         (Some(vec!["foo", "bar"]), pretty_str!(["foo", "bar"])),
//     ]);
// }
//
// #[test]
// fn test_write_newtype_struct() {
//     #[derive(Serialize, PartialEq, Debug)]
//     struct Newtype(BTreeMap<String, i32>);
//
//     let inner = Newtype(treemap!(String::from("inner") => 123));
//     let outer = treemap!(String::from("outer") => to_value(&inner).unwrap());
//
//     test_encode_ok(&[(inner, r#"{"inner":123}"#)]);
//
//     test_encode_ok(&[(outer, r#"{"outer":{"inner":123}}"#)]);
// }
//
// #[test]
// fn test_deserialize_number_to_untagged_enum() {
//     #[derive(Eq, PartialEq, Deserialize, Debug)]
//     #[serde(untagged)]
//     enum E {
//         N(i64),
//     }
//
//     assert_eq!(E::N(0), E::deserialize(Number::from(0)).unwrap());
// }
//
// fn test_parse_ok<T>(tests: Vec<(&str, T)>)
// where
//     T: Clone + Debug + PartialEq + ser::Serialize + de::DeserializeOwned,
// {
//     for (s, value) in tests {
//         let v: T = from_str(s).unwrap();
//         assert_eq!(v, value.clone());
//
//         let v: T = from_slice(s.as_bytes()).unwrap();
//         assert_eq!(v, value.clone());
//
//         // Make sure we can deserialize into a `Value`.
//         let json_value: Value = from_str(s).unwrap();
//         assert_eq!(json_value, to_value(&value).unwrap());
//
//         // Make sure we can deserialize from a `&Value`.
//         let v = T::deserialize(&json_value).unwrap();
//         assert_eq!(v, value);
//
//         // Make sure we can deserialize from a `Value`.
//         let v: T = from_value(json_value.clone()).unwrap();
//         assert_eq!(v, value);
//
//         // Make sure we can round trip back to `Value`.
//         let json_value2: Value = from_value(json_value.clone()).unwrap();
//         assert_eq!(json_value2, json_value);
//
//         // Make sure we can fully ignore.
//         let twoline = s.to_owned() + "\n3735928559";
//         let mut de = Deserializer::from_str(&twoline);
//         IgnoredAny::deserialize(&mut de).unwrap();
//         assert_eq!(0xDEAD_BEEF, u64::deserialize(&mut de).unwrap());
//
//         // Make sure every prefix is an EOF error, except that a prefix of a
//         // number may be a valid number.
//         if !json_value.is_number() {
//             for (i, _) in s.trim_end().char_indices() {
//                 assert!(from_str::<Value>(&s[..i]).unwrap_err().is_eof());
//                 assert!(from_str::<IgnoredAny>(&s[..i]).unwrap_err().is_eof());
//             }
//         }
//     }
// }
//
// // For testing representations that the deserializer accepts but the serializer
// // never generates. These do not survive a round-trip through Value.
// fn test_parse_unusual_ok<T>(tests: Vec<(&str, T)>)
// where
//     T: Clone + Debug + PartialEq + ser::Serialize + de::DeserializeOwned,
// {
//     for (s, value) in tests {
//         let v: T = from_str(s).unwrap();
//         assert_eq!(v, value.clone());
//
//         let v: T = from_slice(s.as_bytes()).unwrap();
//         assert_eq!(v, value.clone());
//     }
// }
//
// macro_rules! test_parse_err {
