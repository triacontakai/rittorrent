use std::collections::HashMap;
use std::hash::Hash;

pub trait RemoveValue<K, V: PartialEq> {
    // returns whether the value was present
    fn remove_value(&mut self, value: V) -> Option<K>;
}

impl<K: Eq + Hash + Clone, V: PartialEq> RemoveValue<K, V> for HashMap<K, V> {
    // surely andrei will replace this horror with 20 magical combined iterator methods
    fn remove_value(&mut self, value: V) -> Option<K> {
        let mut key: Option<K> = None;
        self.retain(|k, v| {
            if *v == value {
                key = Some(k.clone())
            }
            *v != value
        });

        key
    }
}

//#[cfg(test)]
//mod tests {
//
//    use std::collections::HashMap;
//
//    use super::RemoveValue;
//
//    #[test]
//    fn remove_value_test() {
//        let mut map = HashMap::new();
//
//        map.insert(1, 10);
//        map.insert(2, 20);
//        map.insert(3, 30);
//        map.insert(4, 40);
//        map.insert(5, 50);
//
//        assert_eq!(map.get(&3), Some(&30));
//
//        // remove 30
//        assert_eq!(map.remove_value(30), true);
//        assert_eq!(map.remove_value(80), false);
//
//        assert_eq!(map.get(&3), None);
//    }
//}
