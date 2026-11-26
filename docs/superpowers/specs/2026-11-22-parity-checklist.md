# Parity checklist for 27.1.0

Every later slice ticks rows in this list. Per the cross-platform design
(`2026-11-20-cross-platform-27-1-design.md`, section 10), a feature is done
when its row passes on the GPUI app. The baseline is the Swift app at the git
tag `swift-parity-baseline`; each row names something that app does.

How to read it: the **Mac** and **Linux** columns are blank until the row
passes on that platform in the GPUI app; "S1" means slice 1 did it. A row that
cannot pass on Linux at 27.1.0 is listed under "Linux gaps" with its answer.
Row IDs are registry command IDs, window kinds, and sheet or dialog titles.

Command rows are generated: `cargo run -p core-control --example dump_commands`
(238 commands). The "Menu item" column says whether the command has a menu
item (false: keys, the Keys settings and the control channel only).

## Commands

### File

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| file.new | File | New | ⌘N | true | S1 | S1 |
| file.open | File | Open… | ⌘O | true | S1 | S1 |
| file.create_demo | File | Create Demo Project |  | true | S1 | S1 |
| file.create_demo_audio | File | Create Demo Project with Audio |  | true | S7 | S7 |
| media.import | File | Import Audio… | ⇧⌘I | true | S7 | S7 |
| file.close | File | Close | ⌘W | true | S1 | S1 |
| file.save | File | Save | ⌘S | true | S1 | S1 |
| file.save_as | File | Save As… | ⇧⌘S | true | S1 | S1 |
| file.project_settings | File | Project Settings… |  | true | S7 | S7 |
| file.bounce | File | Bounce… |  | true | S7 | S7 |
| file.import_midi | File | Import MIDI File… |  | true | S7 | S7 |
| file.export_midi | File | MIDI File… |  | true | S7 | S7 |
| file.export_midi_clip | File | MIDI Clip File… |  | true | S7 | S7 |
| file.export_for_daw | File | For Another DAW… |  | true | S7 | S7 |
| file.import_dawproject | File | DAWproject… |  | true | S7 | S7 |
| file.export_dawproject | File | DAWproject… |  | true | S7 | S7 |
| file.import_aaf | File | AAF… |  | true | S7 | S7 |
| file.export_aaf | File | AAF… |  | true | S7 | S7 |
| file.interchange_report | File | Last Import or Export Report… |  | true | S7 | S7 |
| media.convert | File | Convert Media to Project Format |  | false | S7 | S7 |
| media.relink | File | Relink Media… |  | true | S7 | S7 |
| file.open_copy | File | Open Co-Producer Copy… |  | true | S7 | S7 |

### Edit

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| edit.undo | Edit | Undo | ⌘Z | true | S2 | S2 |
| edit.redo | Edit | Redo | ⇧⌘Z | true | S2 | S2 |

### View

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| ui.view_arrange | View | Arrange View | ⌃1 | true | S6a | S6a (no key: Linux key scheme pending) |
| ui.view_mix | View | Mix View | ⌃2 | true | S6a | S6a (no key: Linux key scheme pending) |
| ui.toggle_left | View | Left Pane | ⌃⌘← | true | S6a | S6a |
| ui.toggle_bottom | View | Bottom Pane | ⌃⌘↓ | true | S4 | S4 |
| ui.toggle_right | View | Right Pane | ⌃⌘→ | true | S6a | S6a |
| ui.pane | View | Set Pane |  | false | S6a | S6a |
| ui.theme | View | Theme |  | false | S9b | S9b |
| ui.set_transport_bar | View | Set Transport Bar |  | false | S3 | S3 |
| ui.reset_transport_bar | View | Reset Transport Bar |  | false | S3 | S3 |
| ui.customize_transport | View | Customize Transport Bar… |  | true | S9c | S9c |

### Editors

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| editors.piano_roll | Editors | Piano Roll | ⌘1 | true | S5 | S5 |
| editors.list | Editors | List | ⌘2 | true | S4 | S4 |
| editors.strip_chart | Editors | Strip Chart | ⌘3 | true | | |
| editors.drum_grid | Editors | Drum Grid | ⌘4 | true | | |
| editors.audio_editor | Editors | Audio Editor | ⌘5 | true | | |
| editors.piano_roll_window | Editors | Open Piano Roll in Window | ⌥⌘1 | true | | |
| editors.list_window | Editors | Open List in Window | ⌥⌘2 | true | | |
| editors.strip_chart_window | Editors | Open Strip Chart in Window | ⌥⌘3 | true | | |
| editors.drum_grid_window | Editors | Open Drum Grid in Window | ⌥⌘4 | true | | |
| editors.close_detail | Editors | Close Detail Editor |  | true | S4 | S4 |
| editors.insert_event | Editors | Insert Event | ⌘I | true | S4 | S4 |
| editors.nudge_earlier | Editors | Nudge Earlier | ⌥← | true | | |
| editors.nudge_later | Editors | Nudge Later | ⌥→ | true | | |
| editors.nudge_up | Editors | Nudge Up | ⌥↑ | true | | |
| editors.nudge_down | Editors | Nudge Down | ⌥↓ | true | | |
| editors.snap | Editors | Snap to Grid | G | true | S4 | S4 |

### Modify

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| modify.select_by_rule | Modify | Select by Rule… | ⌃⌘S | true | | |
| modify.transpose | Modify | Transpose… | ⌃⌘T | true | | |
| modify.quantize | Modify | Quantize… | ⌃⌘U | true | | |
| modify.change | Modify | Change Values… | ⌃⌘M | true | | |
| modify.move | Modify | Move… | ⌃⌘V | true | | |
| modify.again | Modify | Modify Again | ⌃⌘R | true | | |
| modify.template | Modify | Templates |  | true | | |
| modify.exact_mode | Modify | Exact Mode | ⌃⌘E | true | | |
| modify.quantize_notes | Modify | Quantize Notes |  | false | | |
| modify.transpose_notes | Modify | Transpose Notes |  | false | | |
| modify.velocity_notes | Modify | Change Velocity |  | false | | |

### Sequence

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| sequence.make_reference | Sequence | Make Reference from Selection | ⌥⌘R | true | | |
| sequence.back | Sequence | Back to Parent Sequence | ⌘[ | true | | |

### Region

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| region.normalize | Region | Normalize… |  | true | | |
| region.fade_in_to_cursor | Region | Fade In to Cursor |  | true | | |
| region.fade_out_from_cursor | Region | Fade Out from Cursor |  | true | | |
| region.remove_fades | Region | Remove Fades |  | true | | |
| region.crossfade | Region | Crossfade |  | true | | |
| region.set_fade_shape | Region | Set Fade Shape |  | false | | |
| region.remove_crossfade | Region | Remove Crossfade |  | true | | |
| region.open_external | Region | Open in External Editor |  | true | | |
| region.reload_external | Region | Reload External Edit |  | true | | |

### Drum

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| drum.add_row | Drum | Add Drum Row |  | true | | |
| drum.show_velocity | Drum | Show Velocity |  | true | | |
| drum.split_row | Drum | Split Row to Own Track |  | true | | |
| drum.remove_row | Drum | Remove Drum Row |  | true | | |
| drum.timeline | Drum | Timeline View |  | true | | |

### Transport

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| transport.play_stop | Transport | Play / Stop | Space | true | S1, S3 | S1, S3 |
| transport.stop | Transport | Stop | S | true | S1, S3 | S1, S3 |
| transport.play | Transport | Play |  | false | S3 | S3 |
| transport.play_selection | Transport | Play Selection | ⌥Space | true | S3 | S3 |
| transport.play_from_selection | Transport | Play from Selection | ⇧Space | true | S3 | S3 |
| transport.play_from_edit_in | Transport | Play from Edit-In | Y | true | S3 | S3 |
| transport.record | Transport | Record | R | true | S8a, S8b (R, channel and the Record button; look check open) | S8a, S8b (R, channel and the Record button; look check open) |
| transport.scrub | Transport | Scrub | K | true | S3 | S3 |
| transport.return_to_start | Transport | Return to Start | Return | true | S3 | S3 |
| transport.rewind | Transport | Rewind | ⇧, | true | S3 (keypad key: gap, see below) | S3 (keypad key: gap) |
| transport.fast_forward | Transport | Fast Forward | ⇧. | true | S3 (keypad key: gap, see below) | S3 (keypad key: gap) |
| transport.previous_marker | Transport | Previous Marker | , | true | S3 | S3 |
| transport.next_marker | Transport | Next Marker | . | true | S3 | S3 |
| transport.loop | Transport | Loop | L | true | S3 | S3 |
| transport.overdub | Transport | Overdub | O | true | S8a, S8b (the check mark) | S8a, S8b (the check mark) |
| transport.punch | Transport | Punch | I | true | S8a, S8b (the check mark) | S8a, S8b (the check mark) |
| transport.click | Transport | Click |  | true | S8a, S8b (the check mark) | S8a, S8b (the check mark) |
| transport.set_click_level | Transport | Click Level |  | false | S8a | S8a |
| transport.set_click_mode | Transport | Click Mode |  | false | S8a | S8a |
| transport.go_position | Transport | Go to Position Field | P | true | S3 | S3 |
| transport.go_edit_in | Transport | Go to Edit-In Field | E | true | S3 | S3 |
| transport.go_punch | Transport | Go to Punch Fields | U | true | S8b (U and channel; folded, » opens at Punch In) | S8b (U and channel; folded, » opens at Punch In) |
| transport.arm_previous | Transport | Arm Previous Track | ⌥⇧↑ | true | S8a | S8a |
| transport.arm_next | Transport | Arm Next Track | ⌥⇧↓ | true | S8a | S8a |
| marker.add | Transport | Add Marker |  | false | S8a | S8a |
| marker.rename | Transport | Rename Marker |  | false | S8a | S8a |
| marker.remove | Transport | Remove Marker |  | false | S8a | S8a |

### Step

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| step.entry | Step | Step Entry | T | true | S8a, S8b (key, channel and the Step menu) | S8a, S8b (key, channel and the Step menu) |
| step.whole | Step | Whole Note | 1 | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.half | Step | Half Note | 2 | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.quarter | Step | Quarter Note | 3 | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.eighth | Step | Eighth Note | 4 | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.sixteenth | Step | Sixteenth Note | 5 | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.thirty_second | Step | Thirty-Second Note | 6 | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.dot | Step | Dotted | . | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.triplet | Step | Triplet | / | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.tie | Step | Tie | = | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.rest | Step | Rest | 0 | true | S8a, S8b (key, channel and the Step menu; keypad twin: gap) | S8a, S8b (key, channel and the Step menu; keypad twin: gap) |
| step.delete_last | Step | Delete Last Step | Delete | true | S8a, S8b (key, channel and the Step menu) | S8a, S8b (key, channel and the Step menu) |
| step.notes | Step | Enter Notes |  | false | S8a | S8a |

### Automation

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| automation.mode.off | Automation | Off |  | true | S6b | S6b |
| automation.mode.read | Automation | Read |  | true | S6b | S6b |
| automation.mode.touch | Automation | Touch |  | true | S6b | S6b |
| automation.mode.latch | Automation | Latch |  | true | S6b | S6b |
| automation.mode.write | Automation | Write |  | true | S6b | S6b |
| automation.mode.trim | Automation | Trim |  | true | S6b | S6b |
| automation.suspend | Automation | Suspend Automation |  | true | S6b | S6b |

### Window

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| window.mixer | Window | Mixer | ⌥⌘M | true | S6a | S6a |
| window.plugin_manager | Window | Plugin Manager | ⌥⌘P | true | | |
| window.media_pool | Window | Media Pool |  | true | | |
| window.song_notes | Window | Song Notes |  | true | | |

### Track

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| lane.set_input | Track | Set Track Input |  | false | S8c | S8c |
| lane.set_monitor | Track | Set Track Monitor |  | false | S8c | S8c |
| lane.set_output | Track | Set Track Output |  | false | | |
| lane.set_input_fx | Track | Set Input Effects |  | false | | |
| lane.set_mpe | Track | Set Track MPE |  | false | | |
| lane.track_synth | Track | Track Synth |  | false | | |
| track.new_audio | Track | New Audio Track | ⌥⌘A | true | S2 | S2 |
| track.new_instrument | Track | New Instrument Track | ⌥⌘S | true | S2 | S2 |
| track.new_midi | Track | New MIDI Track | ⌥⌘X | true | S2 | S2 |
| track.duplicate | Track | Duplicate Track |  | true | S2 | S2 |
| track.delete | Track | Delete Track |  | true | S2 | S2 |
| track.rename | Track | Rename Track |  | true | S2 | S2 |
| track.make_mine | Track | Make Mine |  | true | | |
| track.choose_take | Track | Choose Take |  | true | | |
| track.rename_take | Track | Rename Take |  | true | | |
| track.delete_take | Track | Delete Take |  | true | | |
| track.new_plugin_instrument | Track | New Plug-in Instrument Track |  | false | | |

### Mixer

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| automation.set_mode | Mixer | Set Automation Mode |  | false | S6a | S6a |
| automation.gesture | Mixer | Automation Gesture |  | false | S6a | S6a |
| automation.release_all | Mixer | Release Channel Touches |  | false | S6a | S6a |
| automation.write | Mixer | Write Automation |  | false | | |
| mixer.new_bus | Mixer | New Bus |  | false | S6a | S6a |
| mixer.make_mine | Mixer | Make Mine |  | false | | |
| mixer.set_colour | Mixer | Colour |  | false | S6b | S6b |
| mixer.set_strip | Mixer | Set Strip |  | false | S6b | S6b |
| mixer.set_output | Mixer | Set Strip Output |  | false | S6b | S6b |
| mixer.add_send | Mixer | Add Send |  | false | S6b | S6b |
| mixer.set_send | Mixer | Set Send |  | false | S6b | S6b |
| mixer.remove_send | Mixer | Remove Send |  | false | S6b | S6b |
| mixer.add_insert | Mixer | Add Insert |  | false | | |
| mixer.remove_insert | Mixer | Remove Insert |  | false | | |
| mixer.move_insert | Mixer | Move Insert |  | false | | |
| mixer.set_bypass | Mixer | Bypass Insert |  | false | | |
| mixer.set_eq | Mixer | Set EQ |  | false | | |
| mixer.set_param | Mixer | Set Plug-in Parameter |  | false | | |
| mixer.remove_bus | Mixer | Remove Bus |  | false | S6b | S6b |
| mixer.new_group | Mixer | Group |  | true | S6b | S6b |
| mixer.rename_group | Mixer | Rename Group |  | false | S6b | S6b |
| mixer.set_group_colour | Mixer | Group Colour |  | false | S6b | S6b |
| mixer.add_to_group | Mixer | Add to Group |  | false | S6b | S6b |
| mixer.remove_from_group | Mixer | Remove from Group |  | false | S6b | S6b |
| mixer.set_group_links | Mixer | Group Links |  | false | S6b | S6b |
| mixer.delete_group | Mixer | Ungroup |  | true | S6b | S6b |
| mixer.new_vca | Mixer | New VCA |  | true | S6b | S6b |
| mixer.rename_vca | Mixer | Rename VCA |  | false | S6b | S6b |
| mixer.set_vca_colour | Mixer | VCA Colour |  | false | S6b | S6b |
| mixer.assign_vca | Mixer | Assign to VCA |  | false | S6b | S6b |
| mixer.unassign_vca | Mixer | Remove from VCA |  | false | S6b | S6b |
| mixer.set_vca | Mixer | Set VCA |  | false | S6b | S6b |
| mixer.delete_vca | Mixer | Delete VCA |  | false | S6b | S6b |

### Locators

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| locator.go_0 | Locators | Go to Locator 0 | 0 | false | S8a | S8a |
| locator.go_1 | Locators | Go to Locator 1 | 1 | false | S8a | S8a |
| locator.go_2 | Locators | Go to Locator 2 | 2 | false | S8a | S8a |
| locator.go_3 | Locators | Go to Locator 3 | 3 | false | S8a | S8a |
| locator.go_4 | Locators | Go to Locator 4 | 4 | false | S8a | S8a |
| locator.go_5 | Locators | Go to Locator 5 | 5 | false | S8a | S8a |
| locator.go_6 | Locators | Go to Locator 6 | 6 | false | S8a | S8a |
| locator.go_7 | Locators | Go to Locator 7 | 7 | false | S8a | S8a |
| locator.go_8 | Locators | Go to Locator 8 | 8 | false | S8a | S8a |
| locator.go_9 | Locators | Go to Locator 9 | 9 | false | S8a | S8a |
| locator.set_0 | Locators | Set Locator 0 | ⌥0 | false | S8a | S8a |
| locator.set_1 | Locators | Set Locator 1 | ⌥1 | false | S8a | S8a |
| locator.set_2 | Locators | Set Locator 2 | ⌥2 | false | S8a | S8a |
| locator.set_3 | Locators | Set Locator 3 | ⌥3 | false | S8a | S8a |
| locator.set_4 | Locators | Set Locator 4 | ⌥4 | false | S8a | S8a |
| locator.set_5 | Locators | Set Locator 5 | ⌥5 | false | S8a | S8a |
| locator.set_6 | Locators | Set Locator 6 | ⌥6 | false | S8a | S8a |
| locator.set_7 | Locators | Set Locator 7 | ⌥7 | false | S8a | S8a |
| locator.set_8 | Locators | Set Locator 8 | ⌥8 | false | S8a | S8a |
| locator.set_9 | Locators | Set Locator 9 | ⌥9 | false | S8a | S8a |
| locator.clear_0 | Locators | Clear Locator 0 | ⌥⇧0 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_1 | Locators | Clear Locator 1 | ⌥⇧1 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_2 | Locators | Clear Locator 2 | ⌥⇧2 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_3 | Locators | Clear Locator 3 | ⌥⇧3 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_4 | Locators | Clear Locator 4 | ⌥⇧4 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_5 | Locators | Clear Locator 5 | ⌥⇧5 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_6 | Locators | Clear Locator 6 | ⌥⇧6 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_7 | Locators | Clear Locator 7 | ⌥⇧7 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_8 | Locators | Clear Locator 8 | ⌥⇧8 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |
| locator.clear_9 | Locators | Clear Locator 9 | ⌥⇧9 | false | S8a (US layout; see gaps) | S8a (US layout; see gaps) |

### Record

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| record.keep_pass | Record | Keep Loop Pass |  | false | S8a | S8a |
| record.discard_pass | Record | Discard Since Last Keep |  | false | S8a | S8a |
| record.set | Record | Record Options |  | false | S8a | S8a |
| record.recover | Record | Recover Recordings |  | false | S8a | S8a (import; Trash: gap H4) |

### Media

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| media.compact | Media | Compact Media |  | false | | |
| media.rename | Media | Rename Media… |  | false | | |

### MIDI

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| midi.inject | MIDI | Inject MIDI |  | false | S8a | S8a |

### Audio

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| audio.set | Audio | Audio Settings |  | false | S9a | S9a |

### Destination

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| destination.set_return_latency | Destination | Set Return Latency |  | false | | |
| destination.set_sound | Destination | Set Built-in Synth Sound |  | false | | |
| destination.set_midi_mix | Destination | Set MIDI Mix |  | false | | |
| destination.set_mpe | Destination | Set Destination MPE |  | false | | |
| destination.measure_return | Destination | Measure Return Latency |  | false | | |
| destination.add_midi | Destination | New MIDI Destination |  | false | | |
| destination.edit_midi | Destination | Edit Destination |  | false | | |

### Surfaces

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| surface.add | Surfaces | Add Surface |  | false | | |
| surface.remove | Surfaces | Remove Surface |  | false | | |
| surface.inject | Surfaces | Inject Surface Bytes |  | false | | |
| surface.learn | Surfaces | MIDI Learn |  | false | | |
| surface.unlearn | Surfaces | Remove MIDI Learn |  | false | | |

### Settings

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| plugins.new_channel_eq | Settings | Put an EQ on New Channels |  | false | S9c | S9c |
| plugins.install_multiq | Settings | Install Multi-Q for Other Apps |  | false | S9c | S9c (unavailable: no bundled EQ on Linux) |
| plugins.default_eq | Settings | EQ for New Channels |  | false | S9c | S9c |
| settings.co_producer | Settings | Co-Producer |  | false | | |
| settings.co_producer_answer | Settings | Answer Co-Producer Approval |  | false | | |
| settings.co_producer_metrics | Settings | Count Where My Time Goes |  | false | | |
| settings.delete_time_records | Settings | Delete All Time Records |  | false | | |

### Key Bindings

| ID | Menu | Title | Shortcut | Menu item | Mac | Linux |
|---|---|---|---|---|---|---|
| keys.save | Key Bindings | Save Key Bindings… |  | false | S9c | S9c |
| keys.load | Key Bindings | Load Key Bindings… |  | false | S9c | S9c |
| keys.reset | Key Bindings | Reset Key Bindings |  | false | S9c | S9c |

## Windows

Confirmed from the scenes in `app/Sources/VizDAW/VizDAWApp.swift`, the
Window menu in `AppCommands.swift`, and a grep of `showWindow(`, `NSWindow(`
and `NSPanel` in `app/Sources/VizDAW` and `app/Sources/VizDAWEditors`
(AppKit windows: plug-in editors, Built-in Synth, Modify panel, torn-off
editors). The record popover is a popover, not a window.

| Window | Kind | Mac | Linux |
|---|---|---|---|
| Document window (one per project) | `WindowGroup(for: UUID)` | S1 | S1 |
| Torn-off editor: Piano Roll | `DetailEditorKind.pianoRoll` (⌥⌘1) | | |
| Torn-off editor: List | `DetailEditorKind.list` (⌥⌘2) | | |
| Torn-off editor: Strip Chart | `DetailEditorKind.stripChart` (⌥⌘3) | | |
| Torn-off editor: Drum Grid | `DetailEditorKind.drumGrid` (⌥⌘4) | | |
| Plug-in editor window (one per insert or instrument slot) | `PluginWindows.show`, AppKit `NSWindow` (`PluginWindows.swift`) | H2 (CLAP editor and parameter list by e2e; AU and VST3 editors open in `manual-checks.md`) | H2 (CLAP and VST3 X11 editors, parameter list and Built-in Synth by CI e2e under Xvfb; real-session checks in `manual-checks.md`) |
| Built-in Synth window | `BuiltinSynthWindow.swift`, AppKit `NSWindow` | H2 | H2 |
| Modify panel (floating, one per document) | `ModifyPanels.show`, AppKit `NSPanel` (`ModifyPanel.swift`) | | |
| Mixer | `mixer` | S6a + S6b (strips, master, buses, meters, sends, groups, VCAs, slots without editors, MIDI strips) | S6a + S6b (same) |
| Media Pool | `media-pool` | | |
| Plugin Manager | `plugins` | | |
| Song Notes | `window.song_notes` (opens the notes file in the system's editor, not a window of its own) | | |
| Licences | `licences` | | |
| Settings: Appearance | Settings tab | S9b (user themes: later) | S9b (CI; on screen: L27) |
| Settings: Editing | Settings tab | S9c (no consumers yet: backlog) | S9c (CI; on screen: owner check) |
| Settings: Keys | Settings tab | S9c (Learn MIDI: Surfaces slice) | S9c (CI; on screen: L28) |
| Settings: Audio | Settings tab | S9a | S9a (CI; on screen: L26) |
| Settings: Recording | Settings tab | S9a | S9a (CI; on screen: L26) |
| Settings: Automation | Settings tab | S9a | S9a (CI; on screen: L26) |
| Settings: Surfaces | Settings tab | | |
| Settings: Plug-ins | Settings tab | S9c | S9c (CI; no bundled EQ) |
| Settings: Co-Producer | Settings tab | | |

## Sheets and dialogs

From `.sheet(`, `.alert(`, `.confirmationDialog(`, `NSSavePanel` and
`NSOpenPanel` in `app/Sources/VizDAW`. A sheet without a title of its own is
listed by its Swift type. 23 `.sheet(` sites make 22 rows because MIDI Learn
Maps is opened from two places.

### Sheets

| Sheet | Where | Mac | Linux |
|---|---|---|---|
| Reference (`ReferenceSheet`) | document | | |
| Some audio files of this project are missing or changed (`RelinkSheet`) | document | | |
| Bounce | document | | |
| Export for Another DAW | document | | |
| Export DAWproject | document | | |
| Export AAF | document | | |
| Import AAF | document | | |
| Track synth (`TrackSynthSheet`, titled "Track" and the synth name) | document | | |
| New MIDI Destination | document | | |
| Destination editor (`DestinationEditorSheet`) | document | | |
| Co-Producer approval (`CoProducerApprovalSheet`) | document | | |
| Import MIDI File | document | | |
| Export MIDI (`MidiExportSheet`) | document | | |
| Report: Export Report, Merge Report, Import Report (`ReportSheet`) | document | | |
| Project Settings | document | | |
| Merge DAWproject | document | | |
| Customize Transport Bar | document and mixer windows | S9c (on screen: pending) | S9c (CI; on screen: L29) |
| MIDI Learn Maps | mixer and panes (two `.sheet(` sites, one sheet) | | |
| Comment (`CommentSheet`) | sequence sidebar | | |
| Key slot (`KeySlotSheet`) | sequence sidebar | | |
| Remove Old Co-Producer Copies? | Media Pool | | |
| Add Surface | Settings > Surfaces | | |

### Alerts and confirmations

| Alert | Where | Mac | Linux |
|---|---|---|---|
| Recovered Recordings | document | S7b, S8a (`record.recover`; never shown under the channel) | S7b, S8a (Move to Trash: gap H4) |
| Document alert (its title is set per error) | document | | |
| Replace question (MIDI import and export) | MidiSheets | | |
| MIDI Learn alert (title set per error) | MIDI Learn Maps | | |
| Surfaces alert (title set per error) | Settings > Surfaces | | |
| Delete notes and copies? | Settings > Co-Producer | | |
| Delete all time records? | Settings > Co-Producer | | |
| Delete this sequence and every reference to it? | sequence sidebar | | |
| Replace the (EQ name) in your Components folder? | Settings > Plug-ins | S9c | Linux gap (no bundled EQ) |

### File panels

20 `NSOpenPanel()`/`NSSavePanel()` sites make 20 rows; the folder panel
is one site used by four callers.

| Panel | Prompt | Mac | Linux |
|---|---|---|---|
| Open (project, MIDI, DAWproject or AAF): `chooseProject` | Open | S7 (on screen: pending) | S7 (owner's manual check) |
| Open Co-Producer copy: `chooseCoProducerCopy` | Open Copy | S7 (on screen: pending) | S7 (owner's manual check) |
| Import MIDI file: `chooseMidi` | Import | S7 (on screen: pending) | S7 (owner's manual check) |
| Save location: `chooseSaveLocation` | Save | S7 (on screen: pending) | S7 (owner's manual check) |
| Relink audio file: `chooseRelink` | Relink | S7 (on screen: pending) | S7 (owner's manual check) |
| Search folder for missing audio: `chooseFolder` | Search | S7 (on screen: pending) | S7 (owner's manual check) |
| Import audio files: `chooseAudio` | Import | S7 (on screen: pending) | S7 (owner's manual check) |
| Bounce file: `chooseBounceFile` | Bounce | S7 (on screen: pending) | S7 (owner's manual check) |
| Folder panel (shared site `chooseFolder(prompt:message:)`): bounce stems folder, MIDI export folder, export folder for another DAW, AAF export folder | Bounce, Export, Export Here | S7 (on screen: pending) | S7 (owner's manual check) |
| MIDI export file: `chooseMidiExportFile` | Export | S7 (on screen: pending) | S7 (owner's manual check) |
| Import DAWproject: `chooseDawprojectToOpen` | Import | S7 (on screen: pending) | S7 (owner's manual check) |
| Merge DAWproject (choose the original project): `chooseProjectToMerge` | Merge | S7 (on screen: pending) | S7 (owner's manual check) |
| Export DAWproject file: `chooseDawprojectFile` | Export | S7 (on screen: pending) | S7 (owner's manual check) |
| Import AAF: `chooseAafToOpen` | Import | S7 (on screen: pending) | S7 (owner's manual check) |
| Choose the MIDI file for the AAF file: `chooseMidiBesideAaf` | Choose | S7 (on screen: pending) | S7 (owner's manual check) |
| Choose folder for the AAF's missing audio: `chooseAafMediaFolder` | Choose | S7 (on screen: pending) | S7 (owner's manual check) |
| Report sheet Save as Text: `chooseReportText` | Save | S7 (on screen: pending) | S7 (owner's manual check) |
| Choose External Sample Editor | Choose | S7 (on screen: pending) | S7 (owner's manual check) |
| Keys settings: save keys file | (save panel) | S7 (on screen: pending) | S7 (owner's manual check) |
| Keys settings: open keys file | (open panel) | S7 (on screen: pending) | S7 (owner's manual check) |

## Manual checks

Sources: `docs/testing/manual-checks.md` (the open list) and the README
(`app/README.md`, "Manual smoke test", numbered steps). Each heading below is
a group of by-eye steps the GPUI app must pass.

### `docs/testing/manual-checks.md`

| Heading | Mac | Linux |
|---|---|---|
| Co-Producer, time coaching and close prompts (README 395-438) | | |
| Fader groups, VCAs and the transport bar (README 439-456) | | |
| Hand checks from the plan's reviews | | |
| Small fixes (README 457-464) | | |
| Inputs, meters, waveforms and markers (GPUI slice 8c) | | |

### `app/README.md` headings

| Heading | Mac | Linux |
|---|---|---|
| Plugins (plan 3c) | | |
| Editors (plan 4a) | | |
| Piano roll (plan 4b) | | |
| List editor and Select and Modify (plan 4c) | partial S4 (List editor; Select and Modify later) | partial S4 |
| Strip Chart and automation (plan 4d) | | |
| Control channel (plan CC) | | |
| Drum grid (plan 4e) | | |
| Keys and step entry (plan 4f) | | |
| Audio playback (plan 5a) | | |
| Regions in the editors (plan 5b) | | |
| Recording (plan 5c) | | |
| Synth tracking (plan 5c-2) | | |
| Record UI and monitoring (plans 5d-1 and 5d-2) | | |
| Automation recording, the engine (plan 5e-1) | | |
| Automation recording, the UI (plan 5e-2) | | |
| Control surfaces and MIDI learn (plan 5f-1) | | |
| Bounce and export (plan 5f-2) | | |
| External sample editor (plan 5f-3) | | |
| Host-integration keyboards (plan 5f-4) | | |
| Window, main views and panes (plan 7a) | | |
| Themes and the channel strip (plan 7b) | | |
| Audio editor (plan 7c) | | |
| Clean-up (plan 7d) | | |
| Tracks (plan tracks) | | |
| MIDI input effects (plan 5g-1) | | |
| MIDI strips (plan 5g-1b) | | |
| MPE input and output (plan 5g-2) | | |
| MIDI files (plan 6a) | | |
| Export for another DAW (plan 6b) | | |
| Crossfades (plan 6c) | | |
| DAWproject (plan 6d) | | |
| DAWproject merge (plan 6e) | | |
| Fader groups, VCAs and the transport bar (plan groups-vcas-transport) | | |
| Small fixes on `next` (after the beta) | | |

## e2e

The 44 tests in `crates/vizctl/tests/e2e.rs`; each must pass against the GPUI
app (they need a window server and `VIZDAW_E2E_APP`).

| Test | Mac | Linux |
|---|---|---|
| demo_play_screenshot_stop | S3, S4 (List step restored) | S3, S4 (List step restored) |
| click_play_and_press_space | S3 | S3 |
| drum_grid_paints_and_erases_by_element | | |
| keys_drive_step_entry_locators_and_bindings | S4, S8a, S9c (bindings half: `gpui_keys_drive_step_entry_and_locators`) | S4, S8a, S9c (CI run 45876230282) |
| audio_import_plays_and_reports_media | | |
| regions_normalize_relink_and_trash | | |
| records_audio_loop_takes_and_midi | S8a (`gpui_records_audio_loop_takes_and_midi`; `record.recover`: `gpui_record_recover_trash_over_the_channel`) | S8a (offline device, fake input; Trash: gap H4) |
| tracks_the_fake_synth | | |
| arms_meters_counts_in_and_draws_the_take | S8c, S9a (`gpui_arms_meters_counts_in_and_draws_the_take`, with the `audio.set` steps) | S8c, S9a (offline device, fake input, with the `audio.set` steps; CI run 45743593726) |
| touch_writes_a_fader_over_the_channel | S6a (mixer parts) | S6a (mixer parts) |
| drives_a_fader_and_a_button_from_an_mcu | | |
| drives_the_mixer_from_a_kontrol_mk3 | | |
| bounces_the_demo_and_checks_the_file | | |
| edits_a_region_through_the_stub_editor | | |
| window_views_and_panes_over_the_channel | S6a (views and panes; mixer-window step later), S8c (markers step) | S6a (views via ui.view_*; ⌃2 is ⌘2 on Linux), S8c (markers step) |
| themes_and_the_strip_over_the_channel | S9b (`gpui_themes_and_the_strip_over_the_channel`; all steps ported) | S9b (CI run 45762262336) |
| edits_a_region_in_the_audio_editor | | |
| transport_jumps_readouts_and_fields_over_the_channel | S3 (steps 0–6), S8b (step 7) | S3 (steps 0–6); step 7 S8b |
| new_project_tracks_over_the_channel | S3 (passed with the screen locked once the channel draws a fresh frame) | S2 |
| loop_wraps_at_the_end | S3 | S3 |
| arpeggiates_an_injected_chord_while_recording | S8a | S8a |
| midi_mix_reaches_state_get | S6b | S6b |
| records_mpe_as_per_note_expression | | |
| midi_files_round_trip_over_the_channel | | |
| exports_for_another_daw_over_the_channel | | |
| crossfades_over_the_channel | | |
| dawproject_round_trip_over_the_channel | | |
| aaf_round_trip_over_the_channel | | |
| dawproject_merge_over_the_channel | | |
| multiq_install_and_switch_over_the_channel | S9c (`gpui_multiq_install_and_switch_over_the_channel`) | S9c (unavailable branch, CI run 45876230282) |
| default_eq_and_new_bus_over_the_channel | S6b, S9c (default-EQ steps in `gpui_new_bus_and_sends_over_the_channel`) | S6b, S9c (CI run 45876230282) |
| takes_and_layered_midi_loop_over_the_channel | | |
| co_producer_over_mcp | | |
| co_producer_librarian_over_mcp | | |
| co_producer_approval_over_mcp | | |
| co_producer_chores_over_mcp | | |
| co_producer_mixer_over_mcp | | |
| co_producer_creative_over_mcp | | |
| release_fixes_over_the_channel | | |
| co_producer_time_over_mcp | | |
| co_producer_import_midi_is_refused | | |
| control_close_of_a_dirty_song_does_not_prompt | S2 | S2 |
| app_quit_refuses_while_a_song_is_unsaved | S2 | S2 |
| groups_vcas_and_transport_bar_over_the_channel | S6a + S6b (strips, groups and VCAs), S8b (the bar part: `gpui_transport_bar_click_and_command_buttons`) | partial S6a; the bar part S8b |

## Linux gaps

Known gaps from the design's section 2a. Each has an answer; no gap is left
unknown.

| Gap | Answer | Status |
|---|---|---|
| The bundled default EQ is AU-only | A CLAP build of it, or a built-in EQ default on Linux | |
| Co-Producer approvals are stored in the Mac Keychain | The Secret Service password store on Linux | |
| AU inserts in Mac projects | Shown as placeholders that keep their settings (design section 10); no automatic swap | |
| `ui.screenshot` of a native Wayland window (the GPUI pin has no offscreen render off macOS) | X11 `GetImage` of the window (XWayland, Xvfb); a native Wayland window answers `unavailable` "Screenshots need an X11 window on Linux." until GPUI or the compositor gives a capture | |
| Keypad − and + as Rewind and Fast Forward keys, and step entry's keypad twins (Keypad1–6 note values, Keypad. Dotted, Keypad/ Triplet, Keypad+ Tie, Keypad0 Rest) (GPUI at the pin does not tell keypad keys from main keys; Mac too) | The ⇧, and ⇧. keys and the buttons; the number row and `.`, `/`, `=` for step entry (with "reserve number row" on, the note values and Rest have no key: the Step menu and the channel); the control channel's `keypad_minus`/`keypad_plus` work. Closes when GPUI reports the keypad | |
| Media import on Linux (only VizDAW CAF files import, with no resampler: `media.import` answers `failed` "… not a VizDAW CAF file" for WAV/AIFF and "… resampling needs the system importer" across rates; the e2e import steps assert those refusals on Linux) | Host track H4: Linux audio import reads WAV/AIFF and other formats (Symphonia) and adds a resampler | |
| Live audio/MIDI input on Linux (input device list and channel names, duplex PipeWire input, ALSA MIDI input, step entry from a played keyboard) | Host track H3; until then recording on Linux uses the offline device (`VIZDAW_AUDIO=offline`, fake input `VIZDAW_AUDIO_INPUT=fake`) and MIDI by `midi.inject`; the picker offers two unnamed channels until H3 | |
| Move to Trash for recovered recordings (`record.recover action: trash`, and Recovered Recordings' Move to Trash) | Host track H4: Linux Trash support. Until then the file stays where it is (kept for the next open) and the call answers `failed` with the problem line `<path>: not moved to the Trash (this platform has no Trash support yet)`; `gpui_record_recover_trash_over_the_channel` asserts it on Linux | |
