#[macro_export]
macro_rules! cfg_variants {
    // Enum entry
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($body:tt)*
        }
    ) => {
        $crate::cfg_variants! {
            @enum_parse
            [$(#[$meta])*]
            [$vis enum $name]
            []
            $($body)*
        }
    };

    (
        match &$object:ident.$field:ident {
            $($body:tt)*
        }
    ) => {
        $crate::cfg_variants! {
            @match_parse
            [&$object.$field]
            []
            $($body)*
        }
    };

    // Match entry
    (
        match ($value:expr) {
            $($body:tt)*
        }
    ) => {
        $crate::cfg_variants! {
            @match_parse
            [$value]
            []
            $($body)*
        }
    };

    // Finished enum
    (
        @enum_parse
        [$($meta:tt)*]
        [$vis:vis enum $name:ident]
        [$($variants:tt)*]
    ) => {
        $($meta)*
        $vis enum $name {
            $($variants)*
        }
    };

    // Conditional enum group
    (
        @enum_parse
        [$($meta:tt)*]
        [$vis:vis enum $name:ident]
        [$($variants:tt)*]
        #[cfg($condition:meta)] {
            $($group:tt)*
        }
        $($rest:tt)*
    ) => {
        $crate::cfg_variants! {
            @enum_group
            [$($meta)*]
            [$vis enum $name]
            [$($variants)*]
            [$condition]
            [$($rest)*]
            $($group)*
        }
    };

    // Finished conditional enum group
    (
        @enum_group
        [$($meta:tt)*]
        [$vis:vis enum $name:ident]
        [$($variants:tt)*]
        [$condition:meta]
        [$($rest:tt)*]
    ) => {
        $crate::cfg_variants! {
            @enum_parse
            [$($meta)*]
            [$vis enum $name]
            [$($variants)*]
            $($rest)*
        }
    };

    // Conditional enum variant
    (
        @enum_group
        [$($meta:tt)*]
        [$vis:vis enum $name:ident]
        [$($variants:tt)*]
        [$condition:meta]
        [$($rest:tt)*]
        $(#[$variant_meta:meta])*
        $variant:ident $({ $($fields:tt)* })? $(($($tuple:tt)*))? ,
        $($remaining:tt)*
    ) => {
        $crate::cfg_variants! {
            @enum_group
            [$($meta)*]
            [$vis enum $name]
            [
                $($variants)*
                #[cfg($condition)]
                $(#[$variant_meta])*
                $variant $({ $($fields)* })? $(($($tuple)*))?,
            ]
            [$condition]
            [$($rest)*]
            $($remaining)*
        }
    };

    // Normal enum variant
    (
        @enum_parse
        [$($meta:tt)*]
        [$vis:vis enum $name:ident]
        [$($variants:tt)*]
        $(#[$variant_meta:meta])*
        $variant:ident $({ $($fields:tt)* })? $(($($tuple:tt)*))? ,
        $($rest:tt)*
    ) => {
        $crate::cfg_variants! {
            @enum_parse
            [$($meta)*]
            [$vis enum $name]
            [
                $($variants)*
                $(#[$variant_meta])*
                $variant $({ $($fields)* })? $(($($tuple)*))?,
            ]
            $($rest)*
        }
    };

    // Final enum variant without comma
    (
        @enum_parse
        [$($meta:tt)*]
        [$vis:vis enum $name:ident]
        [$($variants:tt)*]
        $(#[$variant_meta:meta])*
        $variant:ident $({ $($fields:tt)* })? $(($($tuple:tt)*))?
    ) => {
        $crate::cfg_variants! {
            @enum_parse
            [$($meta)*]
            [$vis enum $name]
            [
                $($variants)*
                $(#[$variant_meta])*
                $variant $({ $($fields)* })? $(($($tuple)*))?,
            ]
        }
    };

    // Finished match
    (
        @match_parse
        [$value:expr]
        [$($arms:tt)*]
    ) => {
        match $value {
            $($arms)*
        }
    };

    // Conditional match group
    (
        @match_parse
        [$value:expr]
        [$($arms:tt)*]
        #[cfg($condition:meta)] {
            $($group:tt)*
        }
        $($rest:tt)*
    ) => {
        $crate::cfg_variants! {
            @match_group
            [$value]
            [$($arms)*]
            [$condition]
            [$($rest)*]
            $($group)*
        }
    };

    // Finished conditional match group
    (
        @match_group
        [$value:expr]
        [$($arms:tt)*]
        [$condition:meta]
        [$($rest:tt)*]
    ) => {
        $crate::cfg_variants! {
            @match_parse
            [$value]
            [$($arms)*]
            $($rest)*
        }
    };

    // Conditional match arm
    (
        @match_group
        [$value:expr]
        [$($arms:tt)*]
        [$condition:meta]
        [$($rest:tt)*]
        $pattern:pat $(if $guard:expr)? => $result:expr,
        $($remaining:tt)*
    ) => {
        $crate::cfg_variants! {
            @match_group
            [$value]
            [
                $($arms)*
                #[cfg($condition)]
                $pattern $(if $guard)? => $result,
            ]
            [$condition]
            [$($rest)*]
            $($remaining)*
        }
    };

    // Final conditional match arm without comma
    (
        @match_group
        [$value:expr]
        [$($arms:tt)*]
        [$condition:meta]
        [$($rest:tt)*]
        $pattern:pat $(if $guard:expr)? => $result:expr
    ) => {
        $crate::cfg_variants! {
            @match_group
            [$value]
            [
                $($arms)*
                #[cfg($condition)]
                $pattern $(if $guard)? => $result,
            ]
            [$condition]
            [$($rest)*]
        }
    };

    // Normal match arm
    (
        @match_parse
        [$value:expr]
        [$($arms:tt)*]
        $pattern:pat $(if $guard:expr)? => $result:expr,
        $($rest:tt)*
    ) => {
        $crate::cfg_variants! {
            @match_parse
            [$value]
            [
                $($arms)*
                $pattern $(if $guard)? => $result,
            ]
            $($rest)*
        }
    };

    // Final match arm without comma
    (
        @match_parse
        [$value:expr]
        [$($arms:tt)*]
        $pattern:pat $(if $guard:expr)? => $result:expr
    ) => {
        $crate::cfg_variants! {
            @match_parse
            [$value]
            [
                $($arms)*
                $pattern $(if $guard)? => $result,
            ]
        }
    };
}

#[macro_export]
macro_rules! handle_thread_output {
    ($output: expr) => {
        if let $crate::thread::ThreadFlowThread
    }
}
