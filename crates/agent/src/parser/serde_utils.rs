use std::fmt;
use serde::{Serializer, Deserializer};
use serde::ser::SerializeMap;
use serde::de::Visitor;

pub fn serialize_fields_as_map<S>(fields: &[(String, String)], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(fields.len()))?;
    for (k, v) in fields {
        map.serialize_entry(k, v)?;
    }
    map.end()
}

pub fn deserialize_fields_from_map<'de, D>(deserializer: D) -> Result<Vec<(String, String)>, D::Error>
where
    D: Deserializer<'de>,
{
    struct MapVisitor;

    impl<'de> Visitor<'de> for MapVisitor {
        type Value = Vec<(String, String)>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a JSON object")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut fields = Vec::with_capacity(map.size_hint().unwrap_or(0));
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                fields.push((key, value));
            }
            Ok(fields)
        }
    }

    deserializer.deserialize_map(MapVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: serialize fields via serde_json
    fn serialize_fields(fields: &[(String, String)]) -> String {
        use serde::Serialize;

        #[derive(Serialize)]
        struct Wrapper<'a> {
            #[serde(serialize_with = "serialize_fields_as_map")]
            fields: &'a [(String, String)],
        }

        let w = Wrapper { fields };
        serde_json::to_string(&w).unwrap()
    }

    // Helper: deserialize fields via serde_json
    fn deserialize_fields(json: &str) -> Vec<(String, String)> {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "deserialize_fields_from_map")]
            fields: Vec<(String, String)>,
        }

        let w: Wrapper = serde_json::from_str(json).unwrap();
        w.fields
    }

    #[test]
    fn test_serialize_empty_fields() {
        let json = serialize_fields(&[]);
        assert_eq!(json, r#"{"fields":{}}"#);
    }

    #[test]
    fn test_serialize_single_field() {
        let fields = vec![("key".to_string(), "value".to_string())];
        let json = serialize_fields(&fields);
        assert_eq!(json, r#"{"fields":{"key":"value"}}"#);
    }

    #[test]
    fn test_serialize_multiple_fields() {
        let fields = vec![
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ];
        let json = serialize_fields(&fields);
        assert!(json.contains(r#""a":"1""#));
        assert!(json.contains(r#""b":"2""#));
    }

    #[test]
    fn test_deserialize_empty_map() {
        let fields = deserialize_fields(r#"{"fields":{}}"#);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_deserialize_single_field() {
        let fields = deserialize_fields(r#"{"fields":{"key":"value"}}"#);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0], ("key".to_string(), "value".to_string()));
    }

    #[test]
    fn test_round_trip() {
        let original = vec![
            ("host".to_string(), "server-01".to_string()),
            ("level".to_string(), "info".to_string()),
            ("pid".to_string(), "12345".to_string()),
        ];
        let json = serialize_fields(&original);
        let deserialized = deserialize_fields(&json);
        // Note: JSON object key order may differ, so check contents not order
        for (k, v) in &original {
            assert!(deserialized.contains(&(k.clone(), v.clone())),
                "Missing key-value pair: {}={}", k, v);
        }
        assert_eq!(original.len(), deserialized.len());
    }

    #[test]
    fn test_serialize_special_characters() {
        let fields = vec![
            ("path".to_string(), "/api/users?id=123&name=foo".to_string()),
            ("msg".to_string(), "line with \"quotes\" and \\backslashes".to_string()),
        ];
        let json = serialize_fields(&fields);
        // Should be valid JSON
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }
}
