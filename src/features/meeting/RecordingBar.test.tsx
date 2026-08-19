// @vitest-environment jsdom

import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { RecordingBar } from './RecordingBar'

afterEach(cleanup)

describe('RecordingBar', () => {
  it('starts with both sources enabled and disables source changes while recording', async () => {
    const user = userEvent.setup()
    const onStart = vi.fn()
    const onSourcesChange = vi.fn()
    const view = render(
      <RecordingBar
        status="idle"
        sources={{ microphone: true, systemAudio: true }}
        elapsedMs={0}
        busy={false}
        onSourcesChange={onSourcesChange}
        onStart={onStart}
        onPause={vi.fn()}
        onResume={vi.fn()}
        onStop={vi.fn()}
      />,
    )

    await user.click(screen.getByRole('button', { name: '开始录音' }))
    expect(onStart).toHaveBeenCalledOnce()

    view.rerender(
      <RecordingBar
        status="recording"
        sources={{ microphone: true, systemAudio: true }}
        elapsedMs={3_000}
        busy={false}
        onSourcesChange={onSourcesChange}
        onStart={onStart}
        onPause={vi.fn()}
        onResume={vi.fn()}
        onStop={vi.fn()}
      />,
    )

    expect(screen.getByLabelText('麦克风')).toBeDisabled()
    expect(screen.getByLabelText('系统声音')).toBeDisabled()
    expect(screen.getByText('00:00:03')).toBeInTheDocument()
  })

  it('requires at least one source and exposes pause, resume and stop commands', async () => {
    const user = userEvent.setup()
    const onPause = vi.fn()
    const onResume = vi.fn()
    const onStop = vi.fn()
    const props = {
      sources: { microphone: false, systemAudio: false },
      elapsedMs: 0,
      busy: false,
      onSourcesChange: vi.fn(),
      onStart: vi.fn(),
      onPause,
      onResume,
      onStop,
    }
    const view = render(<RecordingBar {...props} status="idle" />)
    expect(screen.getByRole('button', { name: '开始录音' })).toBeDisabled()

    view.rerender(
      <RecordingBar
        {...props}
        status="recording"
        sources={{ microphone: true, systemAudio: true }}
      />,
    )
    await user.click(screen.getByRole('button', { name: '暂停' }))
    await user.click(screen.getByTitle('结束会议'))
    expect(onPause).toHaveBeenCalledOnce()
    expect(onStop).toHaveBeenCalledOnce()

    view.rerender(
      <RecordingBar
        {...props}
        status="paused"
        sources={{ microphone: true, systemAudio: true }}
      />,
    )
    await user.click(screen.getByRole('button', { name: '继续' }))
    expect(onResume).toHaveBeenCalledOnce()
  })
})
