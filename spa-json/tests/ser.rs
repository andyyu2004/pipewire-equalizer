use spa_json::json;

#[test]
fn test() {
    expect_test::expect![[r#"
        {
          "age": 30,
          "is_student": false,
          "address": {
            "street": "123 Main St",
            "city": "Wonderland"
          },
          "courses": [
            "Math",
            "Science",
            "Art"
          ],
          "name": "Alice"
        }"#]]
    .assert_eq(
        &spa_json::to_string_pretty(&json!({
            "name": "Alice",
            "age": 30,
            "is_student": false,
            "courses": ["Math", "Science", "Art"],
            "address": {
                "street": "123 Main St",
                "city": "Wonderland"
            }
        }))
        .unwrap(),
    )
}
