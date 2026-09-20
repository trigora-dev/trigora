import unittest

from trigora import effect, wait_for_event


class AuthoringTests(unittest.TestCase):
    def test_primitives_require_the_cli(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "trigora dev"):
            effect("generate", lambda: 42)
        with self.assertRaisesRegex(RuntimeError, "trigora dev"):
            wait_for_event("approved")


if __name__ == "__main__":
    unittest.main()
