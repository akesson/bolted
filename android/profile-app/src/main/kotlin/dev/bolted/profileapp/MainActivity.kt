package dev.bolted.profileapp

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.lifecycle.viewmodel.compose.viewModel

/**
 * The host. Everything interesting is in [ProfileViewModel] (the as-if-generated shell glue) and
 * [ProfileForm] (the view, which holds no business logic and no constraint literal).
 *
 * `viewModel()` scopes the VM to this Activity's `ViewModelStore`, which **survives a configuration
 * change** and is cleared on a real finish. Since step 05 that is a correctness requirement, not an
 * ergonomic one: on ART the GC never runs a Rust `Drop`, so `onCleared()` is the only place the
 * draft's `close()` can be called (C18).
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                Surface {
                    val vm: ProfileViewModel = viewModel(factory = ProfileViewModel.Factory)
                    ProfileForm(vm)
                }
            }
        }
    }
}
