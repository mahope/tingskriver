package dk.tingskriver.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class InputPolicyTest {
    @Test
    fun allowsPlainMultilineEmailAndWebText() {
        for (type in listOf(0x1, 0x00020001, 0x21, 0xa1)) {
            assertTrue("Text type $type", InputPolicy.allowsDictation(type))
        }
    }

    @Test
    fun refusesAllTextPasswordVariationsEvenWithFlags() {
        for (type in listOf(0x81, 0x91, 0xe1, 0x00080081, 0x000200e1)) {
            assertFalse("Password type $type", InputPolicy.allowsDictation(type))
        }
    }

    @Test
    fun refusesNullNumbersPinsPhoneAndDate() {
        for (type in listOf(0, 0x2, 0x12, 0x3, 0x4)) {
            assertFalse("Non-text type $type", InputPolicy.allowsDictation(type))
        }
    }
}
