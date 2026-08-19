import { X } from 'lucide-react'
import type { ReactNode } from 'react'

type ModalProps = {
  title: string
  children: ReactNode
  onClose: () => void
  footer?: ReactNode
  width?: 'medium' | 'large'
}

export function Modal({
  title,
  children,
  onClose,
  footer,
  width = 'medium',
}: ModalProps) {
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className={`modal-window modal-window--${width}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="modal-header">
          <h2>{title}</h2>
          <button className="icon-button" type="button" onClick={onClose} title="关闭">
            <X aria-hidden="true" />
          </button>
        </header>
        <div className="modal-body">{children}</div>
        {footer ? <footer className="modal-footer">{footer}</footer> : null}
      </section>
    </div>
  )
}
