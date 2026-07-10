package dev.bolted.profileapp

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import com.example.spike_profile_ffi.ping

/**
 * The host. Everything interesting will live in `ProfileViewModel` (the as-if-generated shell glue)
 * and `ProfileForm` (the view, which holds no business logic and no constraint literal).
 *
 * Right now this is the **walking skeleton** (M4): it exists so that step 07's kill criterion 1 —
 * *can a Compose UI suite run on a headless Gradle-Managed Device?* — is answered before anything is
 * built on top of the answer. It also proves the packed `.so` loads inside a real app process, not
 * just an instrumented test process.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Surface { Skeleton() }
            }
        }
    }
}

@Composable
internal fun Skeleton() {
    Text(ping("bolted"), Modifier.testTag("skeleton"))
}
