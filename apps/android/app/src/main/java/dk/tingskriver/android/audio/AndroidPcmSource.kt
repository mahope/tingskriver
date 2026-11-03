package dk.tingskriver.android.audio

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder

class AndroidPcmSource(context: Context) : PcmSource {
    private val recorder: AudioRecord

    init {
        check(context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED)
        val minimum = AudioRecord.getMinBufferSize(16_000, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        check(minimum > 0)
        val candidate = AudioRecord.Builder()
            .setAudioSource(MediaRecorder.AudioSource.MIC)
            .setAudioFormat(AudioFormat.Builder().setSampleRate(16_000)
                .setChannelMask(AudioFormat.CHANNEL_IN_MONO)
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
            .setBufferSizeInBytes(maxOf(minimum, 2560))
            .build()
        try {
            check(candidate.state == AudioRecord.STATE_INITIALIZED)
            candidate.startRecording()
            check(candidate.recordingState == AudioRecord.RECORDSTATE_RECORDING)
            recorder = candidate
        } catch (failure: RuntimeException) {
            candidate.release()
            throw failure
        }
    }

    override fun read(target: ShortArray): Int =
        recorder.read(target, 0, target.size, AudioRecord.READ_NON_BLOCKING)

    override fun close() {
        try { recorder.stop() } finally { recorder.release() }
    }
}
