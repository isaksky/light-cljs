__RNvCsaCpq0navtZb_9tokenizer4scan:
	.cfi_startproc
	cbz	x1, LBB19_19
	sub	sp, sp, #32
	.cfi_def_cfa_offset 32
	mov	x8, x0
	mov	x0, #0
	mov	x12, #0
	add	x9, sp, #8
	mov	x10, #20895015895040
LBB19_2:
	mov	x11, x12
	b	LBB19_4
LBB19_3:
	add	x11, x11, #1
	cmp	x1, x11
	b.eq	LBB19_18
LBB19_4:
	ldrb	w13, [x8, x11]
	sub	w12, w13, #9
	cmp	w12, #5
	b.lo	LBB19_3
	cmp	w13, #44
	b.eq	LBB19_3
	cmp	w13, #32
	b.eq	LBB19_3
	add	x12, x11, #1
	cmp	w13, #41
	b.eq	LBB19_10
	cmp	w13, #40
	b.ne	LBB19_11
	mov	w13, #0
	stp	x11, x12, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x12, x1
	b.lo	LBB19_2
	b	LBB19_18
LBB19_10:
	mov	w13, #1
	stp	x11, x12, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x12, x1
	b.lo	LBB19_2
	b	LBB19_18
LBB19_11:
	cmp	x12, x1
	b.lo	LBB19_13
	b	LBB19_16
LBB19_12:
	add	x12, x12, #1
	mov	w13, #2
	cmp	x1, x12
	b.eq	LBB19_17
LBB19_13:
	ldrb	w13, [x8, x12]
	sub	w14, w13, #9
	cmp	w14, #5
	b.lo	LBB19_16
	cmp	w13, #44
	b.hi	LBB19_12
	lsr	x13, x10, x13
	tbz	w13, #0, LBB19_12
LBB19_16:
	mov	w13, #2
	stp	x11, x12, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x12, x1
	b.lo	LBB19_2
	b	LBB19_18
LBB19_17:
	mov	x12, x1
	stp	x11, x1, [sp, #8]
	strb	w13, [sp, #24]
	; InlineAsm Start
	; InlineAsm End
	add	x0, x0, #1
	cmp	x1, x1
	b.lo	LBB19_2
LBB19_18:
	add	sp, sp, #32
	.cfi_def_cfa_offset 0
	ret
LBB19_19:
	mov	x0, #0
	ret
	.cfi_endproc
