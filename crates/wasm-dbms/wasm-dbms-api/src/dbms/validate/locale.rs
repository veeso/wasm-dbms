use crate::prelude::{Validate, Value};

const ISO_3166_COUNTRIES: &[&str] = &[
    "AD", "AE", "AF", "AG", "AI", "AL", "AM", "AO", "AQ", "AR", "AS", "AT", "AU", "AW", "AX", "AZ",
    "BA", "BB", "BD", "BE", "BF", "BG", "BH", "BI", "BJ", "BL", "BM", "BN", "BO", "BQ", "BR", "BS",
    "BT", "BV", "BW", "BY", "BZ", "CA", "CC", "CD", "CF", "CG", "CH", "CI", "CK", "CL", "CM", "CN",
    "CO", "CR", "CU", "CV", "CW", "CX", "CY", "CZ", "DE", "DJ", "DK", "DM", "DO", "DZ", "EC", "EE",
    "EG", "EH", "ER", "ES", "ET", "FI", "FJ", "FM", "FO", "FR", "GA", "GB", "GD", "GE", "GF", "GG",
    "GH", "GI", "GL", "GM", "GN", "GP", "GQ", "GR", "GS", "GT", "GU", "GW", "GY", "HK", "HM", "HN",
    "HR", "HT", "HU", "ID", "IE", "IL", "IM", "IN", "IO", "IQ", "IR", "IS", "IT", "JE", "JM", "JO",
    "JP", "KE", "KG", "KH", "KI", "KM", "KN", "KP", "KR", "KW", "KY", "KZ", "LA", "LB", "LC", "LI",
    "LK", "LR", "LS", "LT", "LU", "LV", "LY", "MA", "MC", "MD", "ME", "MF", "MG", "MH", "MK", "ML",
    "MM", "MN", "MO", "MP", "MQ", "MR", "MS", "MT", "MU", "MV", "MW", "MX", "MY", "MZ", "NA", "NC",
    "NE", "NF", "NG", "NI", "NL", "NO", "NP", "NR", "NU", "NZ", "OM", "PA", "PE", "PF", "PG", "PH",
    "PK", "PL", "PM", "PN", "PR", "PS", "PT", "PW", "PY", "QA", "RE", "RO", "RS", "RU", "RW", "SA",
    "SB", "SC", "SD", "SE", "SG", "SH", "SI", "SJ", "SK", "SL", "SM", "SN", "SO", "SR", "SS", "ST",
    "SV", "SX", "SY", "SZ", "TC", "TD", "TF", "TG", "TH", "TJ", "TK", "TL", "TM", "TN", "TO", "TR",
    "TT", "TV", "TZ", "UA", "UG", "UM", "US", "UY", "UZ", "VA", "VC", "VE", "VG", "VI", "VN", "VU",
    "WF", "WS", "YE", "YT", "ZA", "ZM", "ZW",
];

const ISO_639_1_COUNTRIES: &[&str] = &[
    "aa", "ab", "ae", "af", "ak", "am", "an", "ar", "as", "av", "ay", "az", "ba", "be", "bg", "bh",
    "bi", "bm", "bn", "bo", "br", "bs", "ca", "ce", "ch", "co", "cr", "cs", "cu", "cv", "cy", "da",
    "de", "dv", "dz", "ee", "el", "en", "eo", "es", "et", "eu", "fa", "ff", "fi", "fj", "fo", "fr",
    "fy", "ga", "gd", "gl", "gn", "gu", "gv", "ha", "he", "hi", "ho", "hr", "ht", "hu", "hy", "hz",
    "ia", "id", "ie", "ig", "ii", "ik", "io", "is", "it", "iu", "ja", "jv", "ka", "kg", "ki", "kj",
    "kk", "kl", "km", "kn", "ko", "kr", "ks", "ku", "kv", "kw", "ky", "la", "lb", "lg", "li", "ln",
    "lo", "lt", "lu", "lv", "mg", "mh", "mi", "mk", "ml", "mn", "mr", "ms", "mt", "my", "na", "nb",
    "nd", "ne", "ng", "nl", "nn", "no", "nr", "nv", "ny", "oc", "oj", "om", "or", "os", "pa", "pi",
    "pl", "ps", "pt", "qu", "rm", "rn", "ro", "ru", "rw", "sa", "sc", "sd", "se", "sg", "si", "sk",
    "sl", "sm", "sn", "so", "sq", "sr", "ss", "st", "su", "sv", "sw", "ta", "te", "tg", "th", "ti",
    "tk", "tl", "tn", "to", "tr", "ts", "tt", "tw", "ty", "ug", "uk", "ur", "uz", "ve", "vi", "vo",
    "wa", "wo", "xh", "yi", "yo", "za", "zh", "zu",
];

/// Validator for ISO3166 country codes.
///
/// # Examples
///
/// ```rust
/// use wasm_dbms_api::prelude::{CountryIso3166Validator, Validate, Value};
///
/// let validator = CountryIso3166Validator;
/// let valid_country = Value::Text("US".into());
/// let invalid_country = Value::Text("XX".into());
///
/// assert!(validator.validate(&valid_country).is_ok());
/// assert!(validator.validate(&invalid_country).is_err());
/// ```
pub struct CountryIso3166Validator;

impl Validate for CountryIso3166Validator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(crate::prelude::DbmsError::Validation(
                "Country ISO3166 validator only works on text values".to_string(),
            ));
        };

        if !ISO_3166_COUNTRIES.contains(&text.as_str()) {
            return Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{}' is not a valid ISO3166 country code",
                text
            )));
        }

        Ok(())
    }
}

/// Validator for ISO639 country codes.
///
/// # Examples
///
/// ```rust
/// use wasm_dbms_api::prelude::{CountryIso639Validator, Validate, Value};
/// let validator = CountryIso639Validator;
/// let valid_country = Value::Text("en".into());
/// let invalid_country = Value::Text("xx".into());
/// assert!(validator.validate(&valid_country).is_ok());
/// assert!(validator.validate(&invalid_country).is_err());
/// ```
pub struct CountryIso639Validator;

impl Validate for CountryIso639Validator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(crate::prelude::DbmsError::Validation(
                "Country ISO639 validator only works on text values".to_string(),
            ));
        };

        if !ISO_639_1_COUNTRIES.contains(&text.as_str()) {
            return Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{}' is not a valid ISO639 country code",
                text
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_country_iso3166_validator() {
        let validator = CountryIso3166Validator;
        let valid_country = Value::Text("US".into());
        let invalid_country = Value::Text("XX".into());
        let wrong_type = Value::Int32(123i32.into());

        assert!(validator.validate(&valid_country).is_ok());
        assert!(validator.validate(&invalid_country).is_err());
        assert!(validator.validate(&wrong_type).is_err());
    }

    #[test]
    fn test_country_iso639_validator() {
        let validator = CountryIso639Validator;
        let valid_country = Value::Text("en".into());
        let invalid_country = Value::Text("xx".into());
        let wrong_type = Value::Int32(123i32.into());
        assert!(validator.validate(&valid_country).is_ok());
        assert!(validator.validate(&invalid_country).is_err());
        assert!(validator.validate(&wrong_type).is_err());
    }
}
