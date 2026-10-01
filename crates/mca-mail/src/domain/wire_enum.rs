/// Generates a string-backed enum used across domain, persistence and API layers.
///
/// The canonical representation is a `TEXT` column holding the snake_case wire
/// value. This keeps migrations tolerant: adding a variant never requires an
/// `ALTER TYPE` on an existing deployment, and unknown values read back from the
/// database degrade gracefully through [`TryFromError`] instead of panicking.
macro_rules! wire_enum {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $wire:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )*
        }

        impl $name {
            /// Every declared variant, in declaration order.
            pub const ALL: &'static [$name] = &[ $( $name::$variant, )* ];

            /// Stable snake_case representation used in API and database rows.
            pub const fn as_str(&self) -> &'static str {
                match self { $( $name::$variant => $wire, )* }
            }

            /// Human readable label for audit logs and the admin API.
            pub const fn label(&self) -> &'static str {
                match self { $( $name::$variant => $wire, )* }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::domain::WireParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s.trim().to_ascii_lowercase().as_str() {
                    $( $wire => Ok($name::$variant), )*
                    other => Err($crate::domain::WireParseError {
                        type_name: stringify!($name),
                        value: other.to_string(),
                    }),
                }
            }
        }

        impl TryFrom<&str> for $name {
            type Error = $crate::domain::WireParseError;

            fn try_from(value: &str) -> Result<Self, <Self as TryFrom<&str>>::Error> {
                value.parse()
            }
        }

        impl TryFrom<String> for $name {
            type Error = $crate::domain::WireParseError;

            fn try_from(value: String) -> Result<Self, <Self as TryFrom<String>>::Error> {
                value.parse()
            }
        }
    };
}

pub(crate) use wire_enum;
