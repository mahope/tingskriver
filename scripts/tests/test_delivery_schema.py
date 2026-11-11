"""Validate the Rust serialization fixture against the published sidecar schema."""
import copy
from datetime import datetime
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator, FormatChecker, ValidationError

ROOT = Path(__file__).resolve().parents[2]
FORMATS = FormatChecker()


@FORMATS.checks("date-time", raises=ValueError)
def date_time(value):
    if not isinstance(value, str):
        return True
    return datetime.fromisoformat(value.replace("Z", "+00:00")).tzinfo is not None


class DeliverySchemaTest(unittest.TestCase):
    def test_documented_delivery_examples(self):
        schema = json.loads((ROOT / "docs/schemas/delivery-v1.schema.json").read_text())
        validator = Draft202012Validator(schema, format_checker=FORMATS)
        examples = ROOT / "docs/examples/aflevering"
        validator.validate(json.loads((examples / "gyldig/sidecar.json").read_text()))
        with self.assertRaises(ValidationError):
            validator.validate(json.loads((examples / "ugyldig/sidecar.json").read_text()))

    def test_schema_accepts_rust_fixture_and_rejects_bad_versions_and_types(self):
        schema = json.loads((ROOT / "docs/schemas/delivery-v1.schema.json").read_text())
        fixture = json.loads((ROOT / "testsaet/delivery/sidecar-v1.json").read_text())
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema, format_checker=FORMATS)
        validator.validate(fixture)
        for key, value in [("kilde", "other"), ("skemaversion", 2),
                           ("titel", "  "), ("tid", "yesterday"),
                           ("talere", "speaker"), ("opgaver", [{}]),
                           ("unexpected", True)]:
            broken = copy.deepcopy(fixture)
            broken[key] = value
            with self.subTest(key=key), self.assertRaises(ValidationError):
                validator.validate(broken)
        for key in fixture:
            broken = copy.deepcopy(fixture)
            del broken[key]
            with self.subTest(missing=key), self.assertRaises(ValidationError):
                validator.validate(broken)


if __name__ == "__main__":
    unittest.main()
