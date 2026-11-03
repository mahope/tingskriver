package dk.tingskriver.android

/** Android InputType bit layout, kept free of Android runtime calls for JVM tests. */
object InputPolicy {
    fun allowsDictation(inputType: Int): Boolean {
        val inputClass = inputType and 0x0000000f
        val variation = inputType and 0x00000ff0
        // Only text; excludes TYPE_NULL, number (including PIN), phone and date.
        if (inputClass != 0x00000001) return false
        // TYPE_TEXT_VARIATION_PASSWORD, VISIBLE_PASSWORD and WEB_PASSWORD.
        return variation !in setOf(0x00000080, 0x00000090, 0x000000e0)
    }
}
